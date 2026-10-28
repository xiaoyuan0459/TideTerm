//! Real loopback SSH/SFTP owns transport limits; Qt owns keyboard/send behavior.
use super::*;
use crate::rust::{
    command_completion::{completion_context, completion_environment_query},
    environment_remote,
    store::{SessionConfig, SessionKind},
    term,
};
use crate::session_config::SshAuth;
use tiderm_core::sftp::{DirectoryQuery, DirectoryReadError};

fn remote_plan(cwd: &str, query: &str) -> Option<environment_remote::Plan> {
    environment_remote::plan(cwd, query, 0)
}
fn request(owner: u64, context: &CompletionContext, query: &str) -> EnvironmentBatch {
    environment_request(owner, context, query, 0)
}
#[test]
fn remote_paths_keep_posix_spelling_and_reject_shell_or_client_path_grammar() {
    for query in ["cat ./ENV_", "cat ../ENV_", "cat /env-a/ENV_", "cat ./中"] {
        assert!(remote_plan("/env-a", query).is_some());
    }
    for query in [
        "cat C:/ENV_",
        "cat \\\\server\\name",
        "cat //server/name",
        "cat './ENV_'",
        "cat ./ENV_;id",
        "cat ./$(id)",
        "cat ./a\n",
        "cat ENV_",
    ] {
        assert!(remote_plan("/env-a", query).is_none(), "{query:?}");
    }
    assert!(remote_plan("C:/client", "cat ./ENV_").is_none());
}

fn query(entries: usize, bytes: usize) -> DirectoryQuery {
    DirectoryQuery {
        max_entries: entries,
        max_name_bytes: bytes,
        deadline: Instant::now() + Duration::from_secs(3),
        canceled: Arc::new(AtomicBool::new(false)),
    }
}
fn ready(owner: u64) -> EnvironmentBatch {
    let result = std::cell::RefCell::new(response(1));
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            *result.borrow_mut() = environment_poll(owner);
            result.borrow().state != 1
        }
    ));
    result.into_inner()
}

#[test]
#[ignore = "requires isolated ssh_sftp_peer.py loopback fixture and TEST_SSH_* configuration"]
fn ssh_paths_use_authenticated_sftp_and_retire_without_local_fallback() {
    let _serial = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    crate::rust::term_ssh_tests::isolate_known_hosts();
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.host = "127.0.0.1".into();
    ssh.port = std::env::var("TEST_SSH_PORT")
        .expect("fixture port")
        .parse()
        .unwrap();
    ssh.username = "fixture".into();
    ssh.password = "fixture-password".into();
    ssh.auth = SshAuth::Password;
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            let c = completion_context(tab);
            c.ready && c.cwd == "/env-a"
        }
    ));
    let protocol = term::try_with_tab(tab, |s| s.host.protocol())
        .flatten()
        .unwrap();
    let channel = crate::rust::session::shared_runtime()
        .block_on(protocol.open_sftp())
        .unwrap();
    let rt = crate::rust::session::shared_runtime();
    let listing = rt
        .block_on(channel.list_dir_bounded("/many", query(17, 262144)))
        .unwrap();
    assert!(listing.truncated && listing.entries.len() == 17);
    let listing = rt
        .block_on(channel.list_dir_bounded("/many", query(2048, 60)))
        .unwrap();
    assert!(listing.truncated && !listing.entries.is_empty());
    assert!(
        listing
            .entries
            .iter()
            .map(|e| e.name.len() + e.path.len())
            .sum::<usize>()
            <= 60
    );
    assert!(listing
        .entries
        .iter()
        .all(|e| e.path.starts_with("/many/p") && !e.path.contains('\\')));
    let mut invalid = query(2049, 60);
    assert_eq!(
        rt.block_on(channel.list_dir_bounded("/many", invalid.clone()))
            .unwrap_err(),
        DirectoryReadError::InvalidRequest
    );
    invalid.max_entries = 1;
    invalid.canceled.store(true, Ordering::Relaxed);
    assert_eq!(
        rt.block_on(channel.list_dir_bounded("/many", invalid))
            .unwrap_err(),
        DirectoryReadError::Cancelled
    );
    let mut expired = query(17, 262144);
    expired.deadline = Instant::now() + Duration::from_millis(30);
    assert_eq!(
        rt.block_on(channel.list_dir_bounded("/slow", expired))
            .unwrap_err(),
        DirectoryReadError::TimedOut
    );
    let canceled = query(17, 262144);
    let flag = canceled.canceled.clone();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        flag.store(true, Ordering::Relaxed);
    });
    assert_eq!(
        rt.block_on(channel.list_dir_bounded("/slow", canceled))
            .unwrap_err(),
        DirectoryReadError::Cancelled
    );
    cancel.join().unwrap();
    // A new operation succeeds after each canceled/expired handle has closed.
    assert!(!rt
        .block_on(channel.list_dir_bounded("/env-a", query(2048, 262144)))
        .unwrap()
        .entries
        .is_empty());
    drop(channel);
    let owner = environment_create();
    let context = completion_context(tab);
    let programs = crate::bridge::completion::completion_ffi::ProgramSource {
        configured: true,
        source: tab,
        generation: context.generation,
        directories: vec!["/programs".into()],
    };
    programs_request(owner, &context, "TIDE", 1, &programs);
    let listed = ready(owner);
    assert_eq!(listed.state, 2);
    assert_eq!(listed.items.len(), 2);
    assert!(listed
        .items
        .iter()
        .all(|i| i.kind == 6 && !i.label.contains("SKIP") && !i.label.contains("DIR")));
    assert!(listed
        .items
        .iter()
        .any(|i| i.command == "'/programs/TIDE space'\\''s'"));
    assert!(completion_environment_query(tab, "TIDE", owner)
        .items
        .iter()
        .any(|i| i.kind == 6));
    request(owner, &context, "cat ./ENV_");
    let result = ready(owner);
    assert_eq!(result.state, 2);
    assert!(result.skipped);
    assert_eq!(
        result
            .items
            .iter()
            .map(|i| (i.command.as_str(), i.kind))
            .collect::<Vec<_>>(),
        [("cat ./ENV_DIR/", 4), ("cat ./ENV_ONLY.txt", 4)]
    );
    assert!(environment_valid(
        owner,
        result.request,
        &context,
        "cat ./ENV_"
    ));
    assert!(completion_environment_query(tab, "cat ./ENV_", owner)
        .items
        .iter()
        .any(|i| i.kind == 4));
    let quoted_query = "cat './QUOTE";
    environment_request(owner, &context, quoted_query, 1);
    let quoted = ready(owner);
    assert_eq!(quoted.state, 2);
    assert_eq!(quoted.items[0].command, "cat './QUOTE space'\\''s.txt'");
    assert!(completion_environment_query(tab, quoted_query, owner)
        .items
        .iter()
        .any(|i| i.kind == 4 && i.command == "cat './QUOTE space'\\''s.txt'"));
    assert!(!environment_valid(
        owner,
        result.request,
        &context,
        "cat ./ENV_"
    ));
    request(owner, &context, "cat ./ENV_");
    assert_eq!(ready(owner).state, 2);
    // Reopening within TTL uses only the same source/query snapshot.
    assert_eq!(request(owner, &context, "cat ./ENV_").state, 2);
    term::try_with_tab(tab, |s| s.config.meta.sftp_enabled = false);
    assert!(!environment_valid(
        owner,
        result.request,
        &context,
        "cat ./ENV_"
    ));
    assert_eq!(environment_poll(owner).state, 9);
    assert_eq!(
        programs_request(owner, &completion_context(tab), "TIDE", 1, &programs).state,
        11
    );
    assert_eq!(
        request(owner, &completion_context(tab), "cat ./ENV_").state,
        11
    );
    term::try_with_tab(tab, |s| s.config.meta.sftp_enabled = true);
    request(owner, &completion_context(tab), "cat /denied/");
    assert_eq!(ready(owner).state, 6);
    request(owner, &completion_context(tab), "cat /many/p");
    let result = ready(owner);
    assert!(
        result.state == 2 && result.truncated && result.items.len() == 50,
        "state={} truncated={} count={}",
        result.state,
        result.truncated,
        result.items.len()
    );
    request(owner, &completion_context(tab), "cat /stall/");
    assert_eq!(ready(owner).state, 7);
    request(owner, &completion_context(tab), "cat ./ENV_");
    assert_eq!(ready(owner).state, 2);
    // Directory changes and close reject a previously delivered remote snapshot.
    crate::rust::term_diag::term_feed_inject(tab, b"\x1b]9;9;/env-b\x07");
    assert_eq!(environment_poll(owner).state, 9);
    request(owner, &completion_context(tab), "cat /slow/");
    environment_cancel(owner);
    assert_eq!(environment_poll(owner).state, 0);
    request(owner, &completion_context(tab), "cat ./ENV_");
    term::term_close_tab(tab);
    assert_eq!(environment_poll(owner).state, 9);
    environment_close(owner);
    cfg.ssh.as_mut().unwrap().username = "fixture-no-sftp".into();
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            let c = completion_context(tab);
            c.ready && c.cwd == "/env-a"
        }
    ));
    let owner = environment_create();
    request(owner, &completion_context(tab), "cat ./ENV_");
    let denied = ready(owner);
    assert_eq!(denied.state, 10);
    assert!(denied.items.is_empty());
    environment_close(owner);
    term::term_close_tab(tab);
}
