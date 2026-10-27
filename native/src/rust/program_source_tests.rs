use super::*;
use crate::rust::{
    command_completion::{completion_context, completion_environment_query},
    command_environment::*,
    store::{SessionConfig, SessionKind},
    term,
};
use std::sync::atomic::Ordering;
use std::time::Duration;

fn file(path: &std::path::Path, executable: bool) {
    std::fs::write(path, b"metadata fixture, not executable content").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )
        .unwrap();
    }
    #[cfg(not(unix))]
    let _ = executable;
}
fn plan(dirs: Vec<String>, prefix: &str) -> Plan {
    Plan {
        directories: directories(&dirs, false).unwrap(),
        prefix: prefix.into(),
        dialect: 2,
        protocol: None,
    }
}

#[test]
fn explicit_program_invocations_run_one_synthetic_binary_in_the_selected_shell() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("program quote's");
    std::fs::create_dir(&directory).unwrap();
    let binary = directory.join(if cfg!(windows) {
        "TIDE_PROBE.exe"
    } else {
        "TIDE_PROBE"
    });
    let compile = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/program_probe.rs"
        ))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "synthetic program compilation failed"
    );
    let shells = if cfg!(windows) {
        vec![(2, "pwsh"), (1, "C:/Program Files/Git/bin/bash.exe")]
    } else {
        vec![(1, "sh")]
    };
    for (dialect, shell) in shells {
        let mut plan = plan(vec![directory.to_str().unwrap().into()], "TIDE");
        plan.dialect = dialect;
        let result = plan.scan(
            Instant::now() + Duration::from_secs(4),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(result.state, 2);
        assert_eq!(result.items.len(), 1);
        let mut command = std::process::Command::new(shell);
        if dialect == 2 {
            command.args(["-NoProfile", "-NonInteractive", "-Command"]);
        } else {
            command.arg("-c");
        }
        let output = command.arg(&result.items[0].command).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "PROGRAM_DIRECTORY_ONCE\n"
        );
    }
}
fn collect(owner: u64) -> EnvironmentBatch {
    let result = std::cell::RefCell::new(EnvironmentBatch::default());
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(4),
        || {
            term::term_pump();
            *result.borrow_mut() = environment_poll(owner);
            result.borrow().state != 1
        }
    ));
    result.into_inner()
}

#[test]
fn explicit_directories_filter_metadata_and_bound_sorted_candidates() {
    let root = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    for dir in [root.path(), second.path()] {
        file(&dir.join("TIDE.exe"), true);
    }
    file(&root.path().join("TIDE_SKIP.txt"), false);
    #[cfg(windows)]
    file(&root.path().join("TIDE_SKIP.cmd"), true);
    std::fs::create_dir(root.path().join("TIDE_FOLDER.exe")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.path().join("TIDE.exe"),
        root.path().join("TIDE_LINK.exe"),
    )
    .unwrap();
    let dirs = vec![
        root.path().to_str().unwrap().into(),
        second.path().to_str().unwrap().into(),
    ];
    let plan = plan(dirs.clone(), "TIDE");
    let canceled = Arc::new(AtomicBool::new(false));
    let result = plan.scan(Instant::now() + Duration::from_secs(10), canceled.clone());
    assert_eq!(result.state, 2);
    assert!(!result.truncated && result.items.len() == 2);
    assert_eq!(
        result
            .items
            .iter()
            .filter(|i| i.label == "TIDE.exe")
            .count(),
        2
    );
    assert!(result.items.iter().all(|i| i.kind == 5
        && i.command.starts_with("& '")
        && !i.label.contains("SKIP")
        && !i.label.contains("FOLDER")
        && !i.label.contains("LINK")));
    assert!(result.items.iter().map(|i| i.command.len()).sum::<usize>() <= 65536);
    assert!(result.items.windows(2).all(|w| w[0].label <= w[1].label));
    for i in 0..2100 {
        file(&root.path().join(format!("TIDE{i:04}.exe")), true);
    }
    let large = plan.scan(Instant::now() + Duration::from_secs(10), canceled.clone());
    assert!(large.truncated && large.items.len() == 50);
    assert!(large.items.windows(2).all(|w| w[0].label <= w[1].label));
    assert_eq!(plan.scan(Instant::now(), canceled.clone()).state, 7);
    canceled.store(true, Ordering::Relaxed);
    assert_eq!(
        plan.scan(Instant::now() + Duration::from_secs(3), canceled)
            .state,
        9
    );
    let mut invalid = dirs.clone();
    invalid.push(root.path().join("missing").to_str().unwrap().into());
    assert_eq!(
        self::plan(invalid, "TIDE")
            .scan(
                Instant::now() + Duration::from_secs(10),
                Arc::new(AtomicBool::new(false))
            )
            .state,
        6
    );
    assert_eq!(directories(&[], false), Err(12));
    assert_eq!(directories(&["relative".into()], false), Err(13));
    assert_eq!(directories(&["C:/client".into()], true), Err(13));
    assert_eq!(directories(&vec![dirs[0].clone(); 9], false), Err(13));
    assert_eq!(
        directories(&[dirs[0].clone(), dirs[0].clone()], false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn program_requests_bind_configuration_to_source_and_cache_and_retire_without_writing() {
    let _serial = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    file(&first.path().join("TIDE.exe"), true);
    file(&second.path().join("TIDE.exe"), true);
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    cfg.local.as_mut().unwrap().working_directory = first.path().to_str().unwrap().into();
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_pump();
            completion_context(tab).ready
                && term::try_with_tab(tab, |s| s.context.cwd_reported).unwrap_or(false)
        }
    ));
    let context = completion_context(tab);
    let owner = environment_create();
    let mut settings = ProgramSource {
        configured: true,
        source: tab,
        generation: context.generation,
        directories: vec![first.path().to_str().unwrap().into()],
    };
    assert_eq!(programs_validate(&context, &settings), 0);
    programs_request(owner, &context, "TIDE", 2, &settings);
    let result = collect(owner);
    assert_eq!(result.state, 2);
    assert_eq!(result.items[0].label, "TIDE.exe");
    assert!(completion_environment_query(tab, "TIDE", owner)
        .items
        .iter()
        .any(|i| i.kind == 5));
    assert_eq!(
        programs_request(owner, &context, "TIDE", 2, &settings).state,
        2
    );
    settings.directories = vec![second.path().to_str().unwrap().into()];
    assert_eq!(
        programs_request(owner, &context, "TIDE", 2, &settings).state,
        1
    );
    let next = collect(owner);
    assert_ne!(next.items[0].command, result.items[0].command);
    assert!(!environment_valid(owner, result.request, &context, "TIDE"));
    assert_eq!(
        programs_request(owner, &context, "cat ./", 2, &settings).state,
        0
    );
    assert_eq!(environment_poll(owner).state, 0);
    settings.generation = context.generation.wrapping_add(1);
    assert_eq!(
        programs_request(owner, &context, "TIDE", 2, &settings).state,
        14
    );
    settings.generation = context.generation;
    settings.source = tab.wrapping_add(100);
    assert_eq!(programs_validate(&context, &settings), 14);
    settings.source = tab;
    settings.configured = false;
    assert_eq!(programs_validate(&context, &settings), 12);
    settings.configured = true;
    programs_request(owner, &context, "TIDE", 2, &settings);
    term::term_close_tab(tab);
    assert_eq!(environment_poll(owner).state, 9);
    environment_close(owner);
}
