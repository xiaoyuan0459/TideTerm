//! Explicit in-terminal tmux actions, scoped to one fresh, empty prompt.
use super::{
    store::SessionKind,
    term::{try_with_tab, TermState},
};
use crate::bridge::reading::reading_ffi::TmuxPreview;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(super) struct Request {
    token: u64,
    generation: u32,
    input: u64,
    output: u64,
    action: u8,
    directory: u64,
    deadline: Instant,
    command: String,
}

fn available(st: &TermState, action: u8) -> bool {
    !st.reading
        && !st.host.modem.busy()
        && st.host.alive()
        && st.send_plan.is_none()
        && super::terminal_completion::empty_prompt_echo_ready(st)
        && (action != 0 || !st.pane.alternate_screen())
        && matches!(st.config.kind, SessionKind::Local | SessionKind::Ssh)
        && st.pane.input_line().is_some()
        && (st.pane.command_input_active()
            && st.pane.command_input_prefix().is_some_and(|s| s.is_empty())
            || st
                .pane
                .command_prefix()
                .or_else(|| {
                    st.pane
                        .alternate_screen()
                        .then(|| st.pane.input_line())
                        .flatten()
                })
                .and_then(|s| super::command_history::prompt_input(&s).map(str::to_owned))
                .is_some_and(|s| s.is_empty()))
}

pub fn tmux_prepare(source: u32, name: &str, action: u8) -> TmuxPreview {
    tmux_cancel(source);
    if super::term_ops::input_locked() {
        return TmuxPreview::default();
    }
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || !matches!(action, 0..=2)
    {
        return TmuxPreview {
            error: "session name: 1-64 ASCII letters, digits, _ or -".into(),
            ..Default::default()
        };
    }
    try_with_tab(source, |st| {
        st.tmux_request = None;
        if !available(st, action) {
            return TmuxPreview {
                error: "requires a live Local/SSH terminal at an empty prompt".into(),
                ..Default::default()
            };
        }
        let command = match action {
            0 => format!("tmux new-session -A -s '{name}'"),
            1 => "tmux detach-client".into(),
            _ => concat!(
                "(umask 077; ulimit -f 524288 || exit; tt_history=$(mktemp) || exit; ",
                "trap 'rm -f -- \"$tt_history\"' EXIT HUP INT TERM; ",
                "tmux capture-pane -p -S - -t \"$TMUX_PANE\" > \"$tt_history\" && ",
                "LESSSECURE=1 LESSOPEN= LESSCLOSE= less -- \"$tt_history\")"
            )
            .into(),
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        st.tmux_request = Some(Request {
            token,
            generation: st.generation,
            input: st.input_revision,
            output: st.preview_revision,
            action,
            directory: st.context.cwd_revision,
            deadline: Instant::now() + Duration::from_secs(30),
            command: command.clone(),
        });
        TmuxPreview {
            token,
            command,
            error: String::new(),
        }
    })
    .unwrap_or_default()
}

pub fn tmux_send(source: u32, token: u64, posix_empty_confirmed: bool) -> String {
    if super::term_ops::input_locked() {
        return "input locked".into();
    }
    try_with_tab(source, |st| {
        let Some(request) = st.tmux_request.take() else {
            return "preview expired".into();
        };
        if !posix_empty_confirmed
            || request.token != token
            || request.generation != st.generation
            || request.input != st.input_revision
            || request.directory != st.context.cwd_revision
            || request.output != st.preview_revision
            || Instant::now() >= request.deadline
            || !available(st, request.action)
        {
            return "prompt or source changed; preview again".into();
        }
        let bytes = format!("{}\r", request.command);
        match st.host.queue_text(bytes.as_bytes()) {
            Ok(()) => {
                super::terminal_completion::record_edit(st, 0, 0, "\0");
                super::command_history::submitted(st, bytes.as_bytes());
                String::new()
            }
            Err(error) => error.into(),
        }
    })
    .unwrap_or_else(|| "source closed".into())
}

pub fn tmux_cancel(source: u32) {
    let _ = try_with_tab(source, |st| {
        st.tmux_request = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::{term, term_actions, term_diag, term_tests};

    #[test]
    fn preview_is_read_only_and_rejects_changes_expiry_and_replay() {
        let _serial = term_tests::input_lock_guard();
        let tab = term::term_new_tab();
        term_actions::term_clear(tab, 2);
        term_diag::term_feed_inject(tab, b"fixture@local:/tmp$ \x1b]133;B\x07");
        let first = tmux_prepare(tab, "fixture", 0);
        assert!(first.token != 0, "{}", first.error);
        assert_eq!(first.command, "tmux new-session -A -s 'fixture'");
        assert!(!tmux_send(tab, first.token, false).is_empty());
        assert!(!tmux_send(tab, first.token, true).is_empty());
        term::with_tab(tab, |st| {
            super::super::terminal_completion::record_edit(st, 14, 0, "")
        });
        assert_eq!(
            tmux_prepare(tab, "fixture", 0).token,
            0,
            "wait for the Enter echo"
        );
        term_diag::term_feed_inject(tab, b"\r\nfixture@local:/tmp$ \x1b]133;B\x07");
        assert_ne!(
            tmux_prepare(tab, "fixture", 0).token,
            0,
            "an echoed empty nested prompt is usable"
        );
        let first = tmux_prepare(tab, "fixture", 0);
        assert!(first.token != 0);
        assert!(!tmux_prepare(tab, "bad;command", 0).error.is_empty());
        assert!(!tmux_send(tab, first.token, true).is_empty());
        for change in 0..5 {
            let preview = tmux_prepare(tab, "fixture", 0);
            assert!(preview.token != 0, "{}", preview.error);
            term::with_tab(tab, |st| match change {
                0 => st.input_revision += 1,
                1 => st.preview_revision += 1,
                2 => st.generation += 1,
                3 => st.context.cwd_revision += 1,
                _ => {
                    st.tmux_request.as_mut().unwrap().deadline =
                        Instant::now() - Duration::from_secs(1)
                }
            });
            assert!(!tmux_send(tab, preview.token, true).is_empty());
            assert!(!tmux_send(tab, preview.token, true).is_empty());
        }
        term::term_close_tab(tab);
        assert_eq!(tmux_prepare(tab, "fixture", 0).token, 0);
    }
}
