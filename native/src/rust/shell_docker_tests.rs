//! Docker owns context/member matching; the shared channel already owns policy retirement.
use super::*;
use crate::rust::{command_completion::*, term};

fn report(tab: u32, context: &str) -> ShellReport {
    let preview = shell_docker_prepare(tab, 1, context);
    assert_eq!(preview.state, 1);
    let row = format!("\"{}\"|\"ui_alpha\"", "a".repeat(64));
    let hex: String = row.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    term::with_tab(tab, |st| {
        st.context.shell.pending.as_mut().unwrap().sent = true;
        assert!(st
            .context
            .shell
            .output(
                format!(
                    "\x1b]777;tideterm-env;4;{};b;0;{context};{hex}\x07",
                    preview.token
                )
                .as_bytes()
            )
            .is_empty());
    });
    shell_report_poll(tab)
}
#[test]
fn docker_candidates_keep_explicit_context_and_reject_changed_members_or_receipts() {
    let _net = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            completion_context(tab).ready
        }
    ));
    let first = report(tab, "fixture");
    let batch = completion_query(tab, "docker logs ui_");
    let id = "a".repeat(64);
    assert_eq!(batch.items.iter().filter(|i| i.kind == 10).count(), 1);
    assert!(batch.shell.command.is_empty());
    assert_eq!(
        completion_docker_draft(&batch.context, &first.token, "docker logs ui_", &id),
        format!("docker --context 'fixture' container logs '{id}'")
    );
    assert!(completion_docker_draft(
        &batch.context,
        &first.token,
        "docker logs ui_",
        &"b".repeat(64)
    )
    .is_empty());
    assert!(
        completion_docker_draft(&batch.context, &first.token, "docker exec ui_", &id).is_empty()
    );
    assert!(completion_git_draft(&batch.context, &first.token, "docker logs ui_", &id).is_empty());
    assert!(shell_resolution_draft(tab, &first.token).is_empty());
    // Explicitly changing the configured context replaces the receipt, even with the same IDs.
    let second = report(tab, "other-context");
    assert!(
        completion_docker_draft(&batch.context, &first.token, "docker logs ui_", &id).is_empty()
    );
    assert!(completion_docker_draft(
        &completion_context(tab),
        &second.token,
        "docker inspect ui_",
        &id
    )
    .contains("'other-context'"));
    term::with_tab(tab, |st| st.context.cwd_revision += 1);
    assert!(!completion_shell_valid(
        &completion_context(tab),
        &second.token
    ));
    let third = report(tab, "fixture");
    term::with_tab(tab, |st| {
        st.context.shell.snapshot.as_mut().unwrap().at -= Duration::from_secs(61)
    });
    assert!(completion_docker_draft(
        &completion_context(tab),
        &third.token,
        "docker logs ui_",
        &id
    )
    .is_empty());
    assert_eq!(shell_docker_prepare(tab, 1, "$(unsafe)").state, 12);
    term::term_close_tab(tab);
}
