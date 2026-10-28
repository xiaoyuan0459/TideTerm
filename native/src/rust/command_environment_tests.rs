use super::*;
use crate::rust::{
    command_completion::{completion_context, completion_environment_query},
    environment_paths,
    store::{SessionConfig, SessionKind},
    term,
};

fn local_plan(cwd: &str, query: &str) -> Option<environment_paths::Plan> {
    environment_paths::plan(cwd, query, 0)
}
fn request(owner: u64, context: &CompletionContext, query: &str) -> EnvironmentBatch {
    environment_request(owner, context, query, 0)
}
fn collect(owner: u64) -> EnvironmentBatch {
    let result = std::cell::RefCell::new(response(1));
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(3),
        || {
            term::term_pump();
            *result.borrow_mut() = environment_poll(owner);
            result.borrow().state != 1
        }
    ));
    result.into_inner()
}

#[test]
fn local_paths_replace_only_the_last_token_and_reject_shell_grammar() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file.txt"), b"not read").unwrap();
    std::fs::write(root.path().join("中文.txt"), b"not read").unwrap();
    std::fs::write(root.path().join("unsafe name.txt"), b"not read").unwrap();
    std::fs::write(root.path().join("unsafe$name.txt"), b"not read").unwrap();
    std::fs::create_dir(root.path().join("folder")).unwrap();
    let cwd = root.path().to_str().unwrap();
    let plan = local_plan(cwd, "echo ./").unwrap();
    let result = environment_paths::scan(&plan, Instant::now() + Duration::from_secs(10), || true);
    assert_eq!(result.state, 2);
    assert!(result.skipped);
    assert_eq!(
        result
            .items
            .iter()
            .map(|i| i.command.as_str())
            .collect::<Vec<_>>(),
        ["echo ./file.txt", "echo ./folder/", "echo ./中文.txt"]
    );
    let plan = local_plan(cwd, "cat ./中").unwrap();
    assert_eq!(
        environment_paths::scan(&plan, Instant::now() + Duration::from_secs(10), || true).items[0]
            .command,
        "cat ./中文.txt"
    );
    for query in [
        "cat relative",
        "cat ./a;whoami",
        "cat ./a | x",
        "cat './a'",
        "cat ./$(id)",
        "cat ./a\n",
        "cat //server/x",
        "cat ",
    ] {
        assert!(local_plan(cwd, query).is_none(), "{query:?}");
    }
    let expired = environment_paths::scan(&plan, Instant::now(), || true);
    assert_eq!(expired.state, 7);
    assert!(expired.items.is_empty());
    assert_eq!(
        environment_paths::scan(&plan, Instant::now() + Duration::from_secs(10), || false).state,
        7
    );
    let missing = local_plan(cwd, "cat ./missing/").unwrap();
    assert_eq!(
        environment_paths::scan(&missing, Instant::now() + Duration::from_secs(10), || true).state,
        6
    );
}

#[test]
fn large_directories_return_bounded_sorted_snapshots() {
    let root = tempfile::tempdir().unwrap();
    for n in 0..2100 {
        std::fs::write(root.path().join(format!("p{n:04}")), b"").unwrap();
    }
    let plan = local_plan(root.path().to_str().unwrap(), "echo ./p").unwrap();
    let result = environment_paths::scan(&plan, Instant::now() + Duration::from_secs(10), || true);
    assert_eq!(result.state, 2);
    assert!(result.truncated);
    assert_eq!(result.items.len(), 50);
    assert!(result.items.windows(2).all(|w| w[0].command < w[1].command));
    assert!(result.items.iter().map(|i| i.command.len()).sum::<usize>() <= 65536);
    let long_query = format!("{} ./p", "x".repeat(3500));
    let plan = local_plan(root.path().to_str().unwrap(), &long_query).unwrap();
    let bytes = environment_paths::scan(&plan, Instant::now() + Duration::from_secs(10), || true);
    assert!(bytes.truncated && !bytes.items.is_empty() && bytes.items.len() < 50);
    assert!(bytes.items.iter().map(|i| i.command.len()).sum::<usize>() <= 65536);
}

#[test]
fn requests_cache_briefly_and_retire_on_cancel_directory_change_and_owner_close() {
    let _serial = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("ENV_ONLY"), b"").unwrap();
    std::fs::write(root.path().join("ENV_quoted name's"), b"").unwrap();
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    cfg.local.as_mut().unwrap().working_directory = root.path().to_string_lossy().into_owned();
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_pump();
            completion_context(tab).ready
                && term::try_with_tab(tab, |s| s.context.cwd_reported).unwrap_or(false)
        }
    ));
    let owner = environment_create();
    assert_ne!(owner, 0);
    let context = completion_context(tab);
    let first = request(owner, &context, "echo ./ENV_");
    assert_eq!(first.state, 1);
    let ready = collect(owner);
    assert_eq!(ready.state, 2);
    assert_eq!(ready.items[0].command, "echo ./ENV_ONLY");
    assert!(environment_valid(
        owner,
        ready.request,
        &context,
        "echo ./ENV_"
    ));
    assert!(completion_environment_query(tab, "echo ./ENV_", owner)
        .items
        .iter()
        .any(|i| i.kind == 3));
    // Same source/query with another syntax cannot reuse the conservative cache.
    assert_eq!(
        environment_request(owner, &context, "echo ./ENV_", 2).state,
        1
    );
    let quoted = collect(owner);
    assert!(quoted
        .items
        .iter()
        .any(|i| i.command == "echo './ENV_quoted name''s'"));
    assert!(completion_environment_query(tab, "echo ./ENV_", owner)
        .items
        .iter()
        .any(|i| i.command == "echo './ENV_quoted name''s'"));
    assert!(!environment_valid(
        owner,
        ready.request,
        &context,
        "echo ./ENV_"
    ));
    assert_eq!(request(owner, &context, "echo ./ENV_").state, 1);
    assert!(!collect(owner)
        .items
        .iter()
        .any(|i| i.command.contains("quoted")));
    std::fs::remove_file(root.path().join("ENV_ONLY")).unwrap();
    let cached = request(owner, &context, "echo ./ENV_");
    assert_eq!(cached.state, 2);
    assert_eq!(cached.items[0].command, "echo ./ENV_ONLY");
    assert_ne!(cached.request, ready.request);
    std::thread::sleep(Duration::from_millis(2100));
    request(owner, &context, "echo ./ENV_");
    assert!(collect(owner).items.is_empty());
    request(owner, &context, "echo ./");
    environment_cancel(owner);
    assert_eq!(environment_poll(owner).state, 0);
    assert!(!completion_environment_query(tab, "echo ./", owner)
        .items
        .iter()
        .any(|i| i.kind == 3));
    request(owner, &context, "echo ./");
    let changed = format!("\x1b]9;9;{}\x07", root.path().join("other").display());
    crate::rust::term_diag::term_feed_inject(tab, changed.as_bytes());
    assert_eq!(environment_poll(owner).state, 9);
    environment_close(owner);
    assert_eq!(environment_poll(owner).state, 0);
    term::term_close_tab(tab);
    let owners: Vec<_> = (0..16).map(|_| environment_create()).collect();
    assert!(owners.iter().all(|&id| id != 0));
    assert_eq!(environment_create(), 0);
    for owner in owners {
        environment_close(owner);
    }
}

#[test]
fn non_local_transport_cannot_use_reported_paths_to_read_local_files() {
    use std::io::Write;
    let _serial = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("LOCAL_SECRET_SENTINEL"), b"").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    cfg.tcp.as_mut().unwrap().port = listener.local_addr().unwrap().port();
    cfg.tcp.as_mut().unwrap().host = "127.0.0.1".into();
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_pump();
            completion_context(tab).ready
        }
    ));
    let (mut peer, _) = listener.accept().unwrap();
    // Exercise the protocol boundary with an actual remote directory notification.
    // On Unix the same path exists locally; Windows receives a POSIX remote path.
    let path = if cfg!(windows) {
        format!("/{}", root.path().to_string_lossy().replace('\\', "/"))
    } else {
        root.path().to_string_lossy().into_owned()
    };
    peer.write_all(format!("\x1b]9;9;{path}\x07").as_bytes())
        .unwrap();
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(3),
        || {
            term::term_pump();
            completion_context(tab).cwd == path
        }
    ));
    let owner = environment_create();
    let denied = request(owner, &completion_context(tab), "echo ./LOCAL_");
    assert_eq!(denied.state, 3);
    assert!(denied.items.is_empty());
    assert!(!completion_environment_query(tab, "echo ./LOCAL_", owner)
        .items
        .iter()
        .any(|i| i.kind == 3));
    environment_close(owner);
    term::term_close_tab(tab);
}
