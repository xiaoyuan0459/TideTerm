//! One explicit send plan per source pane; stable targets never follow focus or sync input.
use super::term::{try_with_tab, TermState, TABS};
use crate::bridge::multiline::multiline_ffi::{SendOptions, SendProgress};
use std::cell::Cell;

pub use super::send_text::multiline_preview;

#[derive(Clone)]
struct Target {
    id: u32,
    generation: u32,
    revision: u64,
}

pub struct Plan {
    token: u64,
    source: Target,
    targets: Vec<Target>,
    paste: u8,
    chunks: Vec<Vec<u8>>,
    sent: usize,
    started: bool,
}

thread_local! { static NEXT: Cell<u64> = const { Cell::new(1) }; }

fn snapshot(id: u32, st: &TermState) -> Target {
    Target {
        id,
        generation: st.generation,
        revision: st.input_revision,
    }
}

fn matches(target: &Target, st: &TermState) -> bool {
    target.generation == st.generation
        && target.revision == st.input_revision
        && st.host.alive()
        && !st.host.modem.busy()
}

fn ready(plan: &Plan) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    TABS.with(|tabs| {
        let tabs = tabs.borrow();
        tabs.get(&plan.source.id)
            .is_some_and(|st| matches(&plan.source, st) && (plan.paste != 0 || !st.reading))
            && plan.targets.iter().all(|t| {
                tabs.get(&t.id).is_some_and(|st| {
                    matches(t, st) && (!st.reading || (plan.paste != 0 && t.id == plan.source.id))
                })
            })
    })
}

/// Called when the editor opens or its scope changes. No input is emitted.
pub fn multiline_begin(source: u32, ids: &[u32], paste: u8) -> u64 {
    if super::term_ops::input_locked()
        || ids.is_empty()
        || ids.len() > 128
        || paste > 1
        || (paste != 0 && ids != [source])
    {
        return 0;
    }
    let source_target = try_with_tab(source, |st| snapshot(source, st));
    let Some(source_target) = source_target else {
        return 0;
    };
    let mut targets = Vec::new();
    for &id in ids {
        if targets.iter().any(|t: &Target| t.id == id) {
            continue;
        }
        let Some(target) = try_with_tab(id, |st| snapshot(id, st)) else {
            return 0;
        };
        targets.push(target);
    }
    let token = NEXT.with(|next| {
        let n = next.get();
        next.set(n.saturating_add(1));
        n
    });
    let plan = Plan {
        token,
        source: source_target,
        targets,
        paste,
        chunks: Vec::new(),
        sent: 0,
        started: false,
    };
    if !ready(&plan) {
        return 0;
    }
    try_with_tab(source, |st| st.send_plan = Some(plan));
    token
}

fn take(source: u32, token: u64) -> Option<Plan> {
    try_with_tab(source, |st| {
        if st.send_plan.as_ref().is_some_and(|p| p.token == token) {
            st.send_plan.take()
        } else {
            None
        }
    })
    .flatten()
}

pub fn multiline_valid(source: u32, token: u64) -> bool {
    let Some(plan) = take(source, token) else {
        return false;
    };
    let valid = ready(&plan);
    if valid {
        try_with_tab(source, |st| st.send_plan = Some(plan));
    }
    valid
}

pub fn multiline_end(source: u32, token: u64) {
    drop(take(source, token));
}

fn progress(plan: &Plan, error: u8) -> SendProgress {
    SendProgress {
        sent: plan.sent as u32,
        total: plan.chunks.len() as u32,
        error,
    }
}
fn expired() -> SendProgress {
    SendProgress {
        sent: 0,
        total: 0,
        error: 1,
    }
}

/// Arm once. Subsequent confirmations cannot rewind or repeat the plan.
pub fn multiline_prepare(
    source: u32,
    token: u64,
    text: &str,
    options: &SendOptions,
) -> SendProgress {
    let Some(mut plan) = take(source, token) else {
        return expired();
    };
    if !ready(&plan) {
        return progress(&plan, 1);
    }
    if plan.started {
        let result = progress(&plan, 1);
        try_with_tab(source, |st| st.send_plan = Some(plan));
        return result;
    }
    let lines = match super::send_text::convert(text, options) {
        Ok(lines) => lines,
        Err(error) => {
            let result = progress(&plan, error);
            try_with_tab(source, |st| st.send_plan = Some(plan));
            return result;
        }
    };
    plan.chunks = if options.paced {
        lines
    } else {
        vec![lines.concat()]
    };
    // Paced sends deliberately execute lines; an aggregate paste honors the app's paste mode.
    if plan.paste != 0
        && !options.paced
        && try_with_tab(source, |st| st.pane.bracketed_paste()).unwrap_or(false)
    {
        plan.chunks[0] = [b"\x1b[200~".as_slice(), &plan.chunks[0], b"\x1b[201~"].concat();
    }
    plan.started = true;
    let result = progress(&plan, 0);
    try_with_tab(source, |st| st.send_plan = Some(plan));
    result
}

/// Progress counts accepted FIFO submissions, not remote command completion.
pub fn multiline_step(source: u32, token: u64) -> SendProgress {
    let Some(mut plan) = take(source, token) else {
        return expired();
    };
    if !plan.started || !ready(&plan) {
        return progress(&plan, 1);
    }
    let bytes = &plan.chunks[plan.sent];
    for target in &plan.targets {
        let queued = try_with_tab(target.id, |st| {
            let result = st.host.queue_text(bytes);
            if result.is_ok() {
                super::command_history::submitted(st, bytes);
            }
            result.is_ok()
        })
        .unwrap_or(false);
        // An earlier recipient may already have accepted this chunk. Never retry implicitly.
        if !queued {
            return progress(&plan, 5);
        }
    }
    plan.sent += 1;
    let result = progress(&plan, 0);
    if plan.paste == 1 {
        try_with_tab(source, |st| super::term_reading::set_reading(st, false));
    }
    if plan.sent < plan.chunks.len() {
        // Explicit paste may have changed the source reading revision after its first line.
        if plan.paste == 1
            && let Some(revision) = try_with_tab(source, |st| st.input_revision)
        {
            plan.source.revision = revision;
            for target in &mut plan.targets {
                if target.id == source {
                    target.revision = revision;
                }
            }
        }
        try_with_tab(source, |st| st.send_plan = Some(plan));
    }
    result
}
