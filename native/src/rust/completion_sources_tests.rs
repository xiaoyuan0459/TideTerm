use super::*;
use crate::rust::{
    store::session_config::{SessionConfig, SessionKind},
    term, term_input, terminal_completion,
};
use std::time::Duration;

// Shared configuration owns view independence. Scanner metadata and terminal
// inserted-byte semantics remain in their existing owner tests.
#[test]
fn session_sources_feed_both_entries_and_retire_inflight_receipts_on_change() {
    let _network = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("TIDE_shared.exe"), b"metadata only").unwrap();
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    cfg.local.as_mut().unwrap().working_directory = root.path().to_str().unwrap().into();
    let source = term::open_tab_with_config(&cfg);
    let other = term::open_tab_with_config(&cfg);
    let wait = |condition: &dyn Fn() -> bool| {
        crate::rust::term_net_tests::wait_until(Duration::from_secs(8), || {
            term::term_pump();
            condition()
        })
    };
    assert!(wait(&|| command_completion::completion_context(source)
        .ready
        && command_completion::completion_context(other).ready));
    let original = command_completion::completion_context(source);
    let programs = ProgramSource {
        configured: true,
        source,
        generation: original.generation,
        directories: vec![root.path().to_str().unwrap().into()],
    };
    assert_eq!(
        completion_sources_set(&original, true, 2, true, &programs),
        0
    );
    assert!(!command_completion::completion_valid(&original));
    let settings = completion_sources_get(source);
    assert!(settings.paths && settings.programs_enabled);
    assert!(!completion_sources_get(other).programs_enabled);
    let current = command_completion::completion_context(source);
    assert_eq!(
        completion_sources_set(&current, true, 2, true, &programs),
        0
    );
    assert_eq!(completion_sources_revision(source), settings.revision);
    let owner = command_environment::environment_create();
    command_environment::programs_request(owner, &current, "TIDE", 2, &settings.programs);
    assert!(wait(
        &|| command_environment::environment_poll(owner).state != 1
    ));
    let sender = command_completion::completion_environment_query(source, "TIDE", owner);
    assert!(sender.items.iter().any(|i| i.label == "TIDE_shared.exe"));
    term_input::term_write(source, b"TIDE");
    assert!(wait(&|| terminal_completion::terminal_completion_query(
        source
    )
    .query
        == "TIDE"));
    assert!(wait(&|| terminal_completion::terminal_completion_query(
        source
    )
    .batch
    .items
    .iter()
    .any(|i| i.label == "TIDE_shared.exe")));
    let terminal = terminal_completion::terminal_completion_query(source);
    let at = terminal
        .batch
        .items
        .iter()
        .position(|i| i.label == "TIDE_shared.exe")
        .unwrap() as u32;
    assert!(terminal.environment_owner != owner && terminal.environment_request != 0);
    // One view's cancellation must not cancel the terminal's independent scan.
    command_environment::environment_close(owner);
    assert!(command_environment::environment_valid(
        terminal.environment_owner,
        terminal.environment_request,
        &terminal.batch.context,
        "TIDE"
    ));
    let current = command_completion::completion_context(source);
    assert_eq!(
        completion_sources_set(&current, false, 2, false, &programs),
        0
    );
    assert!(!terminal_completion::terminal_completion_accept(
        &terminal, at
    ));
    assert!(!command_environment::environment_valid(
        terminal.environment_owner,
        terminal.environment_request,
        &terminal.batch.context,
        "TIDE"
    ));
    assert!(terminal_completion::terminal_completion_query(source)
        .batch
        .items
        .iter()
        .all(|i| i.kind != 5));
    assert_eq!(
        completion_sources_set(
            &command_completion::completion_context(other),
            true,
            2,
            true,
            &programs
        ),
        14
    );
    term::term_close_tab(source);
    term::term_close_tab(other);
    assert_eq!(completion_sources_revision(source), 0);
}
