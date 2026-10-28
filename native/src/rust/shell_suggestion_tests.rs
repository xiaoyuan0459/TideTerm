//! Admission fixtures own Sender cache semantics; interpreter and transport tests
//! separately own the real Shell lookup and byte contracts.
use super::*;
use crate::rust::command_completion::{
    completion_context, completion_query, completion_shell_draft, completion_shell_valid,
};
use crate::rust::term;

fn report(tab: u32, name: &str, kind: &str) -> ShellReport {
    let preview = shell_resolution_prepare(tab, 1, name);
    assert_eq!(preview.state, 1);
    term::with_tab(tab, |st| {
        st.context.shell.pending.as_mut().unwrap().sent = true;
        st.context.shell.output(
            format!(
                "\x1b]777;tideterm-env;2;{};b;{kind};{name};\x07",
                preview.token
            )
            .as_bytes(),
        );
    });
    shell_report_poll(tab)
}

#[test]
fn sender_reads_only_matching_receipts_and_rejects_consumed_overwritten_or_stale_drafts() {
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
    term::with_tab(tab, |st| {
        st.context.history.record("Tide_Suggest");
        for n in 0..80 {
            st.context.history.record(&format!("Tide_History{n}"));
        }
    });
    let ready = report(tab, "Tide_Suggest", "function");
    let batch = completion_query(tab, "tide_");
    assert_eq!(batch.items.len(), 50);
    assert!(batch
        .items
        .iter()
        .any(|item| item.kind == 7 && item.command == "Tide_Suggest"));
    assert_eq!(batch.shell.token, ready.token);
    assert!(batch.shell.command.is_empty() && batch.shell.path.is_empty());
    for query in [
        "Other",
        "Suggest",
        "Tide_ ",
        "echo Tide_",
        "./Tide_",
        "Tide_*",
        "Tide_;x",
    ] {
        let filtered = completion_query(tab, query);
        assert!(filtered.shell.token.is_empty());
        assert!(!filtered.items.iter().any(|item| item.kind == 7));
    }
    assert!(completion_query(other, "Tide_").shell.token.is_empty());
    let exact = completion_query(tab, "Tide_Suggest");
    assert_eq!(
        exact
            .items
            .iter()
            .filter(|i| i.command == "Tide_Suggest")
            .count(),
        1
    );
    assert_eq!(
        exact.items[0].kind, 7,
        "Shell source must survive identical history"
    );
    assert!(completion_shell_draft(&completion_context(other), &ready.token).is_empty());
    assert!(completion_shell_valid(&batch.context, &ready.token));
    assert_eq!(
        completion_shell_draft(&batch.context, &ready.token),
        "Tide_Suggest"
    );
    assert!(!completion_shell_valid(&batch.context, &ready.token));
    assert!(completion_shell_draft(&batch.context, &ready.token).is_empty());
    assert!(!completion_query(tab, "Tide_")
        .items
        .iter()
        .any(|i| i.kind == 7));

    let old = report(tab, "Tide_Suggest", "alias");
    let batch = completion_query(tab, "Tide_");
    let new = report(tab, "Tide_New", "builtin");
    assert!(!completion_shell_valid(&batch.context, &old.token));
    assert!(completion_shell_draft(&batch.context, &old.token).is_empty());
    assert_eq!(
        completion_shell_draft(&completion_context(tab), &new.token),
        "Tide_New"
    );
    for kind in ["missing", "keyword"] {
        let ready = report(tab, "Tide_Missing", kind);
        let batch = completion_query(tab, "Tide_Mi");
        assert_eq!(batch.shell.token, ready.token);
        assert!(!batch.shell.can_fill);
        assert!(!batch.items.iter().any(|i| i.kind == 7));
        assert!(completion_shell_valid(&batch.context, &ready.token));
        assert!(completion_shell_draft(&batch.context, &ready.token).is_empty());
    }
    let ready = report(tab, "Tide_Suggest", "function");
    let batch = completion_query(tab, "Tide_");
    term::with_tab(tab, |st| {
        st.context.shell.snapshot.as_mut().unwrap().at -= Duration::from_secs(61)
    });
    assert!(!completion_shell_valid(&batch.context, &ready.token));
    assert!(completion_shell_draft(&batch.context, &ready.token).is_empty());
    assert!(completion_query(tab, "Tide_").shell.token.is_empty());
    let ready = report(tab, "Tide_Suggest", "function");
    let batch = completion_query(tab, "Tide_");
    term::with_tab(tab, |st| st.context.cwd_revision += 1);
    assert!(!completion_shell_valid(&batch.context, &ready.token));
    assert!(completion_shell_draft(&batch.context, &ready.token).is_empty());
    assert!(completion_query(tab, "Tide_").shell.token.is_empty());
    term::term_close_tab(tab);
    term::term_close_tab(other);
}
