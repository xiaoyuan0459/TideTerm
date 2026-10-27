//! The shared terminal channel owns Git snapshot admission and stale draft guards.
use super::*;
use crate::rust::{command_completion::*, term};

fn admit(tab: u32) -> ShellReport {
    let preview = shell_git_prepare(tab, 1);
    assert_eq!(preview.state, 1);
    term::with_tab(tab, |st| {
        let p = st.context.shell.pending.as_mut().unwrap();
        assert!(!p.sent);
        p.sent = true;
        let hex = |s: &str| {
            s.as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let body = format!(
            "\x1b]777;tideterm-env;3;{};b;0;{};{}\x07",
            preview.token,
            hex("/synthetic/repo.git"),
            hex("refs/heads/main\nrefs/tags/main")
        );
        assert!(st.context.shell.output(body.as_bytes()).is_empty());
    });
    let report = shell_report_poll(tab);
    assert_eq!(report.state, 3);
    report
}

#[test]
fn git_drafts_require_live_membership_context_and_original_deadline() {
    let _network = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    let other = term::term_new_tab();
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            completion_context(tab).ready && completion_context(other).ready
        }
    ));
    let first = admit(tab);
    let batch = completion_query(tab, "git show main");
    assert_eq!(batch.shell.token, first.token);
    assert!(batch.shell.command.is_empty());
    assert_eq!(batch.items.iter().filter(|s| s.kind >= 8).count(), 2);
    for _ in 0..2 {
        assert_eq!(
            completion_git_draft(
                &batch.context,
                &first.token,
                "git show main",
                "refs/tags/main"
            ),
            "git show 'refs/tags/main'"
        );
    }
    for (context, token, query, reference) in [
        (
            completion_context(other),
            first.token.clone(),
            "git show main",
            "refs/tags/main",
        ),
        (
            batch.context.clone(),
            "wrong".into(),
            "git show main",
            "refs/tags/main",
        ),
        (
            batch.context.clone(),
            first.token.clone(),
            "git show main",
            "refs/tags/injected",
        ),
        (
            batch.context.clone(),
            first.token.clone(),
            "git show main; x",
            "refs/tags/main",
        ),
        (
            batch.context.clone(),
            first.token.clone(),
            "git switch main",
            "refs/tags/main",
        ),
    ] {
        assert!(completion_git_draft(&context, &token, query, reference).is_empty());
    }
    assert!(completion_query(other, "git show main")
        .shell
        .token
        .is_empty());
    assert!(shell_resolution_draft(tab, &first.token).is_empty());
    term::with_tab(tab, |st| st.context.history.record("unrelated"));
    assert!(completion_git_draft(
        &batch.context,
        &first.token,
        "git show main",
        "refs/tags/main"
    )
    .is_empty());
    let refreshed = completion_context(tab);
    assert!(
        !completion_git_draft(&refreshed, &first.token, "git show main", "refs/tags/main")
            .is_empty()
    );
    for change in 0..5 {
        let ready = admit(tab);
        let context = completion_context(tab);
        term::with_tab(tab, |st| match change {
            0 => st.context.cwd_revision += 1,
            1 => st.input_revision += 1,
            2 => st.generation += 1,
            3 => st.context.shell.snapshot.as_mut().unwrap().at -= Duration::from_secs(61),
            _ => st.context.shell.clear(),
        });
        assert!(
            completion_git_draft(&context, &ready.token, "git show main", "refs/tags/main")
                .is_empty()
        );
        let fresh = completion_query(tab, "git show main");
        if change == 1 {
            assert_eq!(
                fresh.shell.token, ready.token,
                "ordinary draft edits retain the read-only catalog"
            );
        } else {
            assert!(fresh.shell.token.is_empty());
        }
    }
    let ready = admit(tab);
    shell_resolution_prepare(tab, 1, "Other");
    assert!(completion_shell_valid(
        &completion_context(tab),
        &ready.token
    ));
    let preview = shell_git_prepare(tab, 1);
    term::with_tab(tab, |st| {
        st.context.shell.pending.as_mut().unwrap().deadline = Instant::now();
    });
    assert_eq!(shell_report_poll(tab).state, 4);
    assert_eq!(shell_report_send(tab, &preview.token), 6);
    term::term_close_tab(tab);
    term::term_close_tab(other);
}
