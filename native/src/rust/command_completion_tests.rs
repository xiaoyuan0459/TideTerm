use super::*;

#[test]
fn matching_prefers_prefix_then_recent_history_deduplicates_and_bounds_results() {
    let usage = Default::default();
    let item = |kind, label: &str, command: &str| CommandSuggestion {
        kind,
        label: label.into(),
        command: command.into(),
        ..Default::default()
    };
    let result = ranked(
        [
            item(0, "", "echo git"),
            item(0, "", "git status"),
            item(0, "", "git log"),
            item(0, "", "git status"),
            item(1, "仓库 / 状态", "git status"),
            item(1, "GIT / local", "pwd"),
            item(1, "其他", "ls"),
        ],
        "GiT",
        &usage,
    );
    assert_eq!(
        result
            .iter()
            .map(|s| s.command.as_str())
            .collect::<Vec<_>>(),
        ["git status", "git log", "echo git", "pwd"]
    );
    assert_eq!(result[0].kind, 0);
    assert_eq!(
        ranked([item(1, "工具 / 检查", "echo 中文")], "检查", &usage).len(),
        1
    );
    assert_eq!(
        ranked(
            (0..600).map(|n| item(0, "", &format!("cmd {n}"))),
            "",
            &usage
        )
        .len(),
        50
    );
    let mixed = ranked(
        (0..80)
            .map(|n| item(0, "", &format!("s history{n}")))
            .chain([
                item(1, "personal", "s user"),
                item(2, "", "s rule"),
                item(3, "", "s ./path"),
            ]),
        "s",
        &usage,
    );
    assert_eq!(mixed.len(), 50);
    assert_eq!(
        mixed[..4].iter().map(|s| s.kind).collect::<Vec<_>>(),
        [0, 0, 0, 0]
    );
}

#[test]
fn submitted_usage_weights_frequency_recency_and_directory_without_crossing_shells() {
    let mut history = super::super::command_history::CommandHistory::default();
    history.set_context(2, 1, "C:/first");
    for _ in 0..8 {
        history.record("Get-Content alpha");
    }
    history.record("Get-ChildItem beta");
    let choices = |snapshot: super::super::command_history::WeightedHistory| {
        ranked(
            snapshot
                .commands
                .into_iter()
                .map(|command| CommandSuggestion {
                    kind: 0,
                    command,
                    ..Default::default()
                }),
            "Get-C",
            &snapshot.usage,
        )
        .into_iter()
        .map(|c| c.command)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        choices(history.weighted(2, 1, "C:/first"))[0],
        "Get-Content alpha",
        "repeated use outweighs a single newer command"
    );
    history.set_context(2, 1, "C:/second");
    history.record("Get-ChildItem local");
    assert_eq!(
        choices(history.weighted(2, 1, "C:/second"))[0],
        "Get-ChildItem local",
        "same-directory use is preferred"
    );
    assert!(
        history.weighted(1, 2, "/home/lab").commands.is_empty(),
        "PowerShell/Windows history cannot seed Bash/Linux"
    );
    assert!(
        !history.weighted(0, 0, "").commands.is_empty(),
        "an expired remote hint must not hide this terminal's history"
    );
    history.clear();
    assert!(history.weighted(2, 1, "C:/second").commands.is_empty());
}

#[test]
fn snapshots_isolate_panes_and_retire_on_history_mode_lock_reconnect_or_close() {
    use crate::rust::{term, term_lifecycle, term_ops, term_reading};
    use std::time::Duration;
    let _serial = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let first = term::term_new_tab();
    let second = term::term_new_tab();
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_pump();
            completion_context(first).ready && completion_context(second).ready
        }
    ));
    term::with_tab(first, |st| st.context.history.record("echo ONLY_FIRST"));
    let original = command_library::command_catalog();
    let mut entries = original.entries.clone();
    entries.push(crate::bridge::completion::completion_ffi::UserCommand {
        id: 999,
        group: "test".into(),
        name: "template".into(),
        command: "echo {{value}}".into(),
        is_template: true,
    });
    assert_eq!(
        command_library::command_catalog_save(original.revision, &entries),
        0
    );
    let template_context = completion_context(first);
    assert_eq!(
        crate::rust::command_template::command_template_render(
            &template_context,
            999,
            &["中文".into()]
        )
        .text,
        "echo 中文"
    );
    entries.last_mut().unwrap().name = "changed".into();
    assert_eq!(
        command_library::command_catalog_save(template_context.catalog_revision, &entries),
        0
    );
    assert_eq!(
        crate::rust::command_template::command_template_render(
            &template_context,
            999,
            &["late".into()]
        )
        .error,
        1
    );
    let current = command_library::command_catalog();
    assert_eq!(
        command_library::command_catalog_save(current.revision, &original.entries),
        0
    );
    assert_eq!(completion_query(first, "ONLY_FIRST").items.len(), 1);
    assert!(completion_query(second, "ONLY_FIRST").items.is_empty());
    let before = completion_query(first, "").context;
    assert!(completion_valid(&before));
    term::with_tab(first, |st| st.context.history.clear());
    assert!(!completion_valid(&before));
    let before = completion_query(first, "").context;
    term_reading::term_set_reading(first, true);
    assert!(completion_query(first, "").items.is_empty());
    term_reading::term_set_reading(first, false);
    assert!(!completion_valid(&before));
    let before = completion_query(first, "").context;
    term_ops::term_set_locked(true);
    assert!(completion_query(first, "").items.is_empty());
    assert_eq!(command_library::command_catalog_save(1, &[]), 5);
    term_ops::term_set_locked(false);
    assert!(!completion_valid(&before));
    let before = completion_query(first, "").context;
    term_lifecycle::term_disconnect_tab(first);
    assert!(!completion_valid(&before));
    term_lifecycle::term_reconnect_tab(first);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_pump();
            completion_context(first).ready
        }
    ));
    assert!(!completion_valid(&before));
    term::term_close_tab(first);
    term::term_close_tab(second);
    assert!(!completion_valid(&before));
    assert!(completion_query(first, "").items.is_empty());
}
