//! Explicit real-server acceptance of Native file jobs and the transfer queue.
//! Loopback tests own injected failures; this journey owns OpenSSH interoperability.
use super::*;
use crate::rust::{events, file_jobs, local_files, term, term_net_tests, term_ssh_tests};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Session(u32, String, u64);
impl Drop for Session {
    fn drop(&mut self) {
        local_files::files_close(self.2);
        sftp_close(&self.1);
        term::term_close_tab(self.0);
    }
}

fn job(owner: u64, id: u64) -> crate::bridge::files::files_ffi::FileTask {
    let end = Instant::now() + Duration::from_secs(45);
    loop {
        let task = file_jobs::files_task_poll(owner, id);
        if task.state != 1 {
            return task;
        }
        assert!(Instant::now() < end, "real SFTP file job timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn transfer(id: &str) -> sftp_model::TransferItem {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let row = transfers_snapshot()
            .into_iter()
            .find(|item| item.id == id)
            .unwrap();
        if row.completed {
            return row;
        }
        assert!(Instant::now() < end, "real SFTP transfer timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn transfer_id(uid: &str, remote: &str) -> String {
    transfers_snapshot()
        .into_iter()
        .rev()
        .find(|row| row.session_id == uid && row.remote_path == remote)
        .unwrap()
        .id
}

#[test]
#[ignore = "requires trusted TEST_SSH_* and private TEST_SSH_FIXTURE_ROOT; real SFTP writes"]
fn live_daily_file_workflows_preserve_bytes_and_edit_conflicts() {
    let _serial = term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    let parent = std::env::var("TEST_SSH_FIXTURE_ROOT").expect("private remote root required");
    let suffix = parent.strip_prefix("/tmp/tideterm-daily-").unwrap();
    assert!(suffix.len() == 12 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    let (config, _) = term_ssh_tests::ssh_cfg_from_env();
    let tab = term::open_tab_with_config(&config);
    let uid = term::term_uid(tab);
    let owner = local_files::files_create();
    let _session = Session(tab, uid.clone(), owner);
    assert!(term_net_tests::wait_until(Duration::from_secs(35), || {
        term_net_tests::settle(tab);
        if events::take_prompt().is_some() {
            let _ = events::answer_host_key(&uid, false);
            panic!("real SFTP requires the preverified host key");
        }
        term::protocol_of_uid(&uid).is_some_and(|protocol| protocol.is_alive())
    }));
    sftp_open(&uid);
    assert!(term_net_tests::wait_until(Duration::from_secs(20), || {
        channel_ready(&uid)
    }));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let remote = format!("{parent}/workflow-{}-{nonce}", std::process::id());
    let run = |op, src_uid: &str, src: &str, dst_uid: &str, dst: &str, policy| {
        job(
            owner,
            file_jobs::files_task_start(owner, op, src_uid, src, dst_uid, dst, policy).unwrap(),
        )
    };
    let success = |task: crate::bridge::files::files_ffi::FileTask| {
        assert_eq!(task.state, 2, "{}", task.error);
        task
    };
    // mkdir is exclusive: a collision cannot turn an old directory into test data.
    success(run(0, "", "", &uid, &remote, 0));
    let local = tempfile::tempdir().unwrap();
    let source = local.path().join("source");
    std::fs::create_dir_all(source.join("nested/empty")).unwrap();
    std::fs::write(source.join("edit.txt"), b"initial synthetic content").unwrap();
    let binary = [0, 255, 17, 32, 10];
    std::fs::write(source.join("nested/中文 space's.bin"), binary).unwrap();
    let uploaded = format!("{remote}/uploaded");
    success(run(4, "", source.to_str().unwrap(), &uid, &uploaded, 0));
    let download = local.path().join("download");
    success(run(4, &uid, &uploaded, "", download.to_str().unwrap(), 0));
    assert!(download.join("nested/empty").is_dir());
    assert_eq!(
        std::fs::read(download.join("nested/中文 space's.bin")).unwrap(),
        binary
    );
    for (op, from, to) in [
        (0, String::new(), format!("{remote}/new-dir")),
        (1, String::new(), format!("{remote}/new-dir/new.txt")),
        (
            2,
            format!("{remote}/new-dir/new.txt"),
            format!("{remote}/new-dir/moved.txt"),
        ),
    ] {
        success(run(op, &uid, &from, &uid, &to, 0));
    }
    let edit_path = format!("{uploaded}/edit.txt");
    let prepared = success(run(6, &uid, &edit_path, "", "", 0));
    std::fs::write(&prepared.path, b"edited through Native file workflow").unwrap();
    success(run(7, "", &prepared.edit.to_string(), "", "", 0));
    let check = local.path().join("edited.txt");
    success(run(4, &uid, &edit_path, "", check.to_str().unwrap(), 0));
    assert_eq!(
        std::fs::read(&check).unwrap(),
        b"edited through Native file workflow"
    );
    let independent = local.path().join("independent.txt");
    let server_content = b"independent server content with a different length";
    std::fs::write(&independent, server_content).unwrap();
    success(run(
        4,
        "",
        independent.to_str().unwrap(),
        &uid,
        &edit_path,
        2,
    ));
    std::fs::write(
        &prepared.path,
        b"second local edit must not overwrite remote",
    )
    .unwrap();
    let conflict = run(7, "", &prepared.edit.to_string(), "", "", 0);
    assert_eq!(conflict.state, 3, "remote edit conflict must be rejected");
    let preserved = local.path().join("preserved.txt");
    success(run(4, &uid, &edit_path, "", preserved.to_str().unwrap(), 0));
    assert_eq!(std::fs::read(preserved).unwrap(), server_content);
    crate::rust::remote_edit::files_edit_close(owner, prepared.edit, true).unwrap();

    // Pause/resume and cancel use the production queue, not injected transport state.
    let queued = local.path().join("queue.bin");
    let bytes: Vec<u8> = (0..8 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(&queued, &bytes).unwrap();
    let queued_remote = format!("{remote}/queue.bin");
    sftp_upload(&uid, queued.to_str().unwrap(), &remote);
    let id = transfer_id(&uid, &queued_remote);
    assert!(pause_transfer(&id));
    assert_eq!(transfer(&id).error.as_deref(), Some("transfer paused"));
    assert!(resume_transfer(&id));
    assert!(transfer(&id).error.is_none());
    let queue_download = local.path().join("queue-download");
    std::fs::create_dir(&queue_download).unwrap();
    sftp_download(
        &uid,
        &queued_remote,
        queue_download.to_str().unwrap(),
        bytes.len() as u64,
    );
    let id = transfer_id(&uid, &queued_remote);
    assert!(transfer(&id).error.is_none());
    assert_eq!(
        std::fs::read(queue_download.join("queue.bin")).unwrap(),
        bytes
    );
    let cancelled = local.path().join("cancel.bin");
    std::fs::write(&cancelled, b"cancelled synthetic payload").unwrap();
    let cancelled_remote = format!("{remote}/cancel.bin");
    sftp_upload(&uid, cancelled.to_str().unwrap(), &remote);
    let id = transfer_id(&uid, &cancelled_remote);
    assert!(cancel_transfer(&id));
    assert_eq!(transfer(&id).error.as_deref(), Some("transfer cancelled"));
    let channel = with_channel(&uid, Arc::clone).unwrap();
    let runtime = crate::rust::session::shared_runtime();
    let listing = runtime.block_on(channel.list_dir(&remote)).unwrap();
    assert!(!listing
        .iter()
        .any(|entry| entry.name.ends_with(".part") || entry.name == "cancel.bin"));
    success(run(3, &uid, &remote, "", "", 0));
    assert!(runtime.block_on(channel.stat(&remote)).is_err());
    println!("[SFTP-DAILY] recursive/Unicode/rename/edit/conflict/pause/resume/cancel/byte roundtrip/cleanup passed");
}
