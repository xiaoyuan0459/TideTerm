//! Literal, active-terminal draft filling. Never execute or broadcast a suggestion.
use super::term::TermState;
use super::{command_completion as catalog, term::try_with_tab};
use crate::bridge::completion::completion_ffi::TerminalCompletion;

pub struct PendingInput {
    expected: Option<String>,
    output: u64,
    generation: u32,
}

/// Explicit Shell actions can recognize a nested POSIX prompt independently of
/// the launcher's completion dialect, but must still wait for pending input echo.
pub(super) fn empty_prompt_echo_ready(st: &TermState) -> bool {
    st.terminal_input.as_ref().is_none_or(|pending| {
        pending.generation != st.generation
            || pending.expected.as_deref() == Some("")
            || (pending.expected.is_none() && st.preview_revision != pending.output)
    })
}

fn visible_input(st: &TermState) -> Option<String> {
    use super::store::session_config::SessionKind;
    if st.reading
        || st.send_plan.is_some()
        || st.host.modem.busy()
        || !st.host.alive()
        || !matches!(
            st.config.kind,
            SessionKind::Local | SessionKind::Ssh | SessionKind::Telnet
        )
        || st.pane.selection_count() != 0
    {
        return None;
    }
    if st.pane.input_line().is_none()
        && !(st.pane.has_command_input_markers() && st.pane.command_input_active())
    {
        return None;
    }
    let line = st.pane.command_prefix()?;
    let shell = super::completion_shell::configured_kind(st);
    let query = if st.pane.has_command_input_markers() {
        if !st.pane.command_input_active() {
            return None;
        }
        // ConPTY can repaint a conventional prompt after a resize without
        // replaying OSC B. Its visible delimiter is authoritative only while
        // the explicit input lease is still live.
        (shell != 0)
            .then(|| super::command_history::prompt_input_for_shell(&line, shell))
            .flatten()
            .map(str::to_owned)
            .or_else(|| st.pane.command_input_prefix())?
    } else {
        super::command_history::prompt_input_for_shell(&line, shell)?.to_owned()
    };
    (query.len() <= 4096 && !query.chars().any(char::is_control)).then_some(query)
}

fn draft(source: u32) -> Option<(String, u64)> {
    try_with_tab(source, |st| {
        if st
            .terminal_input
            .as_ref()
            .is_some_and(|p| p.generation != st.generation)
        {
            st.terminal_input = None;
        }
        restore_redrawn_empty_prompt(st);
        let query = visible_input(st)?;
        if let Some(pending) = &st.terminal_input {
            let acknowledged = match &pending.expected {
                Some(expected) => query == *expected,
                None => query.is_empty() && st.preview_revision != pending.output,
            };
            if !acknowledged {
                return None;
            }
            st.terminal_input = None;
        }
        Some((query, st.preview_revision))
    })
    .flatten()
}

/// Track edits only at a recognized, visible prompt. This bounded transient
/// draft never records arbitrary key streams or survives the terminal session.
pub(super) fn record_edit(st: &mut TermState, key: u32, mods: u32, text: &str) {
    restore_redrawn_empty_prompt(st);
    let expected = visible_input(st).and_then(|visible| {
        let mut next = st
            .terminal_input
            .as_ref()
            .filter(|p| p.generation == st.generation)
            .and_then(|p| p.expected.clone())
            .unwrap_or(visible);
        if mods & 6 != 0 {
            return None;
        }
        if key == 11 && next.is_ascii() {
            next.pop();
        } else if key == 0 && !text.is_empty() && !text.chars().any(char::is_control) {
            next.push_str(text);
        } else {
            return None;
        }
        (next.len() <= 4096).then_some(next)
    });
    st.terminal_input = Some(PendingInput {
        expected,
        output: st.preview_revision,
        generation: st.generation,
    });
    st.input_revision = st.input_revision.wrapping_add(1);
    st.context.shell.clear_request();
    st.send_plan = None;
}

fn restore_redrawn_empty_prompt(st: &mut TermState) {
    if st.reading
        || st.send_plan.is_some()
        || st.terminal_input.is_some()
        || st.host.modem.busy()
        || !st.host.alive()
        || st.pane.selection_count() != 0
        || super::completion_shell::configured_kind(st) != 2
        || st.pane.command_prefix().as_deref() != Some(" ")
        || st
            .pane
            .command_input_prefix()
            .is_some_and(|text| !text.chars().any(char::is_control))
    {
        return;
    }
    let Some((first, rows)) = st.pane.live_input_physical_rows() else {
        return;
    };
    if rows.last().map(String::as_str) != Some(" ") {
        return;
    }
    for start in (0..rows.len().saturating_sub(1)).rev() {
        if !rows[start].starts_with("PS ") {
            continue;
        }
        let mut prompt: String = rows[start..rows.len() - 1]
            .iter()
            .map(|row| row.trim_end())
            .collect();
        prompt.push(' ');
        if super::command_history::prompt_input_for_shell(&prompt, 2) == Some("")
            && st.pane.restore_live_prompt_at_cursor(first + start as i32)
        {
            st.input_revision = st.input_revision.wrapping_add(1);
            break;
        }
    }
}

pub fn terminal_completion_query(source: u32) -> TerminalCompletion {
    let input = draft(source);
    let (query, output_revision) = input.clone().unwrap_or_default();
    let context = catalog::completion_context(source);
    let (environment_owner, environment) = if input.is_some() && !query.trim().is_empty() {
        super::completion_sources::terminal_request(source, &query, &context)
    } else {
        super::completion_sources::terminal_completion_cancel(source);
        (0, Default::default())
    };
    let mut batch = catalog::completion_environment_query(source, &query, environment_owner);
    if input.is_none() {
        batch.context.ready = false;
        batch.items.clear();
    }
    // Parameter dialogs belong to the Sender. Non-ASCII drafts can only append
    // an exact prefix; never guess a shell editor's grapheme deletion behavior.
    batch.items.retain(|item| {
        !item.is_template
            && (!query.trim().is_empty() || item.kind == 0)
            && !item.command.chars().any(char::is_control)
            && (query.is_ascii() || item.command.starts_with(&query))
    });
    TerminalCompletion {
        environment_owner,
        environment_request: environment.request,
        environment_state: environment.state,
        environment_truncated: environment.truncated,
        environment_skipped: environment.skipped,
        batch,
        query,
        output_revision,
    }
}

pub fn terminal_completion_accept(snapshot: &TerminalCompletion, index: u32) -> bool {
    if !catalog::completion_valid(&snapshot.batch.context)
        || draft(snapshot.batch.context.source)
            != Some((snapshot.query.clone(), snapshot.output_revision))
    {
        return false;
    }
    let Some(item) = snapshot.batch.items.get(index as usize) else {
        return false;
    };
    if item.is_template {
        return false;
    }
    if (3..=6).contains(&item.kind)
        && !super::command_environment::environment_valid(
            snapshot.environment_owner,
            snapshot.environment_request,
            &snapshot.batch.context,
            &snapshot.query,
        )
    {
        return false;
    }
    let context = &snapshot.batch.context;
    let receipt = &snapshot.batch.shell.token;
    let value = match item.kind {
        12 => catalog::completion_name_draft(context, receipt, &item.reference),
        7 => catalog::completion_shell_draft(context, receipt),
        8 | 9 => catalog::completion_git_draft(context, receipt, &snapshot.query, &item.reference),
        10 => catalog::completion_docker_draft(context, receipt, &snapshot.query, &item.reference),
        11 => catalog::completion_kube_draft(context, receipt, &snapshot.query, &item.reference),
        _ => item.command.clone(),
    };
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return false;
    }
    let prefix = value.starts_with(&snapshot.query);
    if !prefix && !snapshot.query.is_ascii() {
        return false;
    }
    try_with_tab(context.source, |st| {
        let mut bytes = Vec::new();
        if prefix {
            bytes.extend_from_slice(&value.as_bytes()[snapshot.query.len()..]);
        } else {
            bytes.resize(
                snapshot.query.len(),
                if st.config.meta.options.backspace_del {
                    127
                } else {
                    8
                },
            );
            bytes.extend_from_slice(value.as_bytes());
        }
        if !bytes.is_empty() && st.host.queue_text(&bytes).is_err() {
            return false;
        }
        st.terminal_input = Some(PendingInput {
            expected: Some(value),
            output: st.preview_revision,
            generation: st.generation,
        });
        st.input_revision = st.input_revision.wrapping_add(1);
        st.context.shell.clear_request();
        st.send_plan = None;
        st.cursor.reset();
        true
    })
    .unwrap_or(false)
}
