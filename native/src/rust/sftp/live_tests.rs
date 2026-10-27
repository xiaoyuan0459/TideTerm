//! Explicit live benchmark of the Qt shell's transfer queue and adaptive pool.

use super::*;
use crate::rust::events;
use crate::rust::store::session_config::{SessionConfig, SessionKind, SshAuth};
use crate::rust::term;
use crate::rust::term_net_tests::{net_serial, settle};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires TEST_SSH_HOST/USER/PASS and a trusted host key; writes 10000 files"]
fn live_ten_thousand_file_upload_uses_adaptive_pool() {
    crate::rust::term_ssh_tests::isolate_known_hosts();
    let _serial = net_serial();
    let host = std::env::var("TEST_SSH_HOST").expect("TEST_SSH_HOST required");
    let user = std::env::var("TEST_SSH_USER").expect("TEST_SSH_USER required");
    let password = std::env::var("TEST_SSH_PASS").expect("TEST_SSH_PASS required");
    let port = std::env::var("TEST_SSH_PORT")
        .unwrap_or_else(|_| "22".into())
        .parse()
        .expect("valid TEST_SSH_PORT");
    let mut config = SessionConfig::blank(SessionKind::Ssh);
    let ssh = config.ssh.as_mut().expect("SSH config");
    ssh.host = host.clone();
    ssh.port = port;
    ssh.username = user.clone();
    ssh.auth = SshAuth::Password;
    ssh.password = password;
    ssh.connect_timeout_secs = 30;

    let tab = term::open_tab_with_config(&config);
    println!("QT_SFTP_TEST_PID={}", std::process::id());
    let uid = format!("tab-{tab}");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !term::protocol_of_uid(&uid).is_some_and(|protocol| protocol.is_alive()) {
        settle(tab);
        if events::take_prompt().is_some() {
            let _ = events::answer_host_key(&uid, false);
            panic!("live SFTP benchmark requires a previously trusted host key");
        }
        assert!(Instant::now() < deadline, "SSH connection timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    sftp_open(&uid);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !channel_ready(&uid) {
        assert!(Instant::now() < deadline, "SFTP channel timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    let channel = with_channel(&uid, |channel| channel.clone()).expect("primary channel");
    let file_count: usize = std::env::var("TEST_SFTP_FILE_COUNT")
        .unwrap_or_else(|_| "10000".into())
        .parse()
        .expect("valid TEST_SFTP_FILE_COUNT");
    let root = format!("/tmp/tideterm-sftp-qt-{}", std::process::id());
    let runtime = crate::rust::session::shared_runtime();
    runtime
        .block_on(channel.mkdir(&root))
        .expect("remote mkdir");
    let local = tempfile::tempdir().expect("local tempdir");
    let started = Instant::now();
    for index in 0..file_count {
        let path = local.path().join(format!("{index:05}.bin"));
        std::fs::File::create(&path)
            .expect("local file")
            .set_len(25 * 1024)
            .expect("local size");
        sftp_upload(&uid, path.to_str().expect("UTF-8 temp path"), &root);
        if (index + 1) % 1000 == 0 {
            println!(
                "QT_ENQUEUED count={} seconds={:.1}",
                index + 1,
                started.elapsed().as_secs_f64()
            );
        }
    }
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut peak_connections = 1;
    let mut cleanup_channels = vec![channel.clone()];
    let mut last_report = Instant::now();
    while transfer_index::desired(&uid) > 0 {
        let pool = transfer_pools().lock().unwrap();
        if pool
            .get(&uid)
            .is_some_and(|channels| channels.len() > peak_connections)
        {
            cleanup_channels = pool[&uid].clone();
            peak_connections = cleanup_channels.len();
        }
        drop(pool);
        if last_report.elapsed() >= Duration::from_secs(5) {
            let queue = transfers().lock().unwrap();
            let finished = queue
                .iter()
                .filter(|item| item.session_id == uid && item.completed)
                .count();
            let failed = queue
                .iter()
                .filter(|item| item.session_id == uid && item.error.is_some())
                .count();
            println!("QT_PROGRESS finished_visible={finished} failed_visible={failed} pending_demand={} pool={} seconds={:.1}",
                transfer_index::desired(&uid), peak_connections, started.elapsed().as_secs_f64());
            last_report = Instant::now();
        }
        assert!(Instant::now() < deadline, "10000-file queue did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed = started.elapsed();
    let entries = runtime
        .block_on(channel.list_dir(&root))
        .expect("remote list");
    assert_eq!(entries.len(), file_count);
    assert!(entries.iter().all(|entry| entry.size_bytes == 25 * 1024));
    assert!(peak_connections > 1, "adaptive pool did not expand");
    assert!(
        transfers_snapshot()
            .iter()
            .filter(|item| item.session_id == uid)
            .all(|item| item.completed && item.error.is_none()),
        "one or more visible transfer rows failed"
    );
    println!(
        "QT_SFTP count={file_count} seconds={:.1} files_per_sec={:.1} peak_connections={peak_connections}",
        elapsed.as_secs_f64(),
        file_count as f64 / elapsed.as_secs_f64()
    );

    if std::env::var_os("TEST_SFTP_CLEANUP_WITH_OPENSSH").is_some() {
        let status = std::process::Command::new("ssh.exe")
            .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes"])
            .arg("-p")
            .arg(port.to_string())
            .arg("-T")
            .arg(format!("{user}@{host}"))
            .arg(format!("rm -rf -- {root}"))
            .status()
            .expect("OpenSSH cleanup command");
        assert!(status.success(), "OpenSSH cleanup failed");
    } else {
        runtime.block_on(async {
            let mut next = 0usize;
            let mut tasks = tokio::task::JoinSet::new();
            while next < file_count || !tasks.is_empty() {
                while next < file_count && tasks.len() < cleanup_channels.len() {
                    let channel = cleanup_channels[next % cleanup_channels.len()].clone();
                    let path = format!("{root}/{next:05}.bin");
                    tasks.spawn(async move { channel.remove_file(&path).await });
                    next += 1;
                }
                tasks
                    .join_next()
                    .await
                    .expect("cleanup task")
                    .expect("cleanup join")
                    .expect("remote remove");
            }
            channel.remove_dir(&root).await.expect("remote rmdir");
        });
    }
    sftp_close(&uid);
    term::term_close_tab(tab);
}
