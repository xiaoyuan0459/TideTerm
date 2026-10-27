use super::file_jobs::*;
use super::store::session_config::{SessionConfig, SessionKind};
use super::{local_files, sftp, term};
use std::time::{Duration, Instant};

fn wait(owner: u64, id: u64) -> crate::bridge::files::files_ffi::FileTask {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let result = files_task_poll(owner, id);
        if result.state != 1 {
            return result;
        }
        assert!(Instant::now() < end, "file task timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires isolated daily_sftp_peer.py loopback fixture"]
fn daily_file_workflows_roundtrip_over_sftp_and_reject_edit_conflicts() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    super::term_ssh_tests::isolate_known_hosts();
    let root = std::path::PathBuf::from(std::env::var("TEST_SSH_KNOWN_HOSTS").unwrap())
        .parent()
        .unwrap()
        .to_owned();
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.host = "127.0.0.1".into();
    ssh.port = std::env::var("TEST_SSH_PORT").unwrap().parse().unwrap();
    ssh.username = "fixture".into();
    ssh.password = "fixture-password".into();
    let tab = term::open_tab_with_config(&cfg);
    let uid = term::term_uid(tab);
    let end = Instant::now() + Duration::from_secs(20);
    while !term::protocol_of_uid(&uid).is_some_and(|p| p.is_alive()) {
        super::term_net_tests::settle(tab);
        assert!(Instant::now() < end, "controlled SSH did not connect");
        std::thread::sleep(Duration::from_millis(10));
    }
    sftp::sftp_open(&uid);
    while !sftp::channel_ready(&uid) {
        assert!(Instant::now() < end, "controlled SFTP did not open");
        std::thread::sleep(Duration::from_millis(10));
    }
    let owner = local_files::files_create();
    let source = root.join("source");
    let download = root.join("download");
    std::fs::create_dir_all(source.join("nested/empty")).unwrap();
    std::fs::write(source.join("file.txt"), b"initial content").unwrap();
    std::fs::write(source.join("nested/中文.bin"), [0, 255, 17]).unwrap();
    let task = wait(
        owner,
        files_task_start(owner, 4, "", source.to_str().unwrap(), &uid, "/uploaded", 0).unwrap(),
    );
    assert_eq!(task.state, 2, "{}", task.error);
    let task = wait(
        owner,
        files_task_start(
            owner,
            4,
            &uid,
            "/uploaded",
            "",
            download.to_str().unwrap(),
            0,
        )
        .unwrap(),
    );
    assert_eq!(task.state, 2, "{}", task.error);
    assert!(download.join("nested/empty").is_dir());
    assert_eq!(
        std::fs::read(download.join("nested/中文.bin")).unwrap(),
        [0, 255, 17]
    );
    for (op, source, target) in [
        (0, "", "/new-dir"),
        (1, "", "/new-dir/new.txt"),
        (2, "/new-dir/new.txt", "/new-dir/moved.txt"),
        (5, "/uploaded", "/new-dir/link"),
    ] {
        let task = wait(
            owner,
            files_task_start(owner, op, &uid, source, &uid, target, 0).unwrap(),
        );
        assert_eq!(task.state, 2, "{}", task.error);
    }
    let blocked = wait(
        owner,
        files_task_start(
            owner,
            4,
            &uid,
            "/new-dir",
            "",
            root.join("blocked").to_str().unwrap(),
            0,
        )
        .unwrap(),
    );
    assert_eq!(blocked.state, 3);
    assert!(!root.join("blocked").exists());
    let prepared = wait(
        owner,
        files_task_start(owner, 6, &uid, "/uploaded/file.txt", "", "", 0).unwrap(),
    );
    assert_eq!(prepared.state, 2, "{}", prepared.error);
    std::fs::write(&prepared.path, b"edited locally").unwrap();
    let upload = wait(
        owner,
        files_task_start(owner, 7, "", &prepared.edit.to_string(), "", "", 0).unwrap(),
    );
    assert_eq!(upload.state, 2, "{}", upload.error);
    assert_eq!(
        std::fs::read(root.join("files/uploaded/file.txt")).unwrap(),
        b"edited locally"
    );
    std::fs::write(
        root.join("files/uploaded/file.txt"),
        b"changed independently on server",
    )
    .unwrap();
    std::fs::write(&prepared.path, b"second local edit").unwrap();
    let conflict = wait(
        owner,
        files_task_start(owner, 7, "", &prepared.edit.to_string(), "", "", 0).unwrap(),
    );
    assert_eq!(conflict.state, 3);
    assert_eq!(
        std::fs::read(root.join("files/uploaded/file.txt")).unwrap(),
        b"changed independently on server"
    );
    std::fs::write(root.join("files/rollback.txt"), b"keep original").unwrap();
    let rollback = wait(
        owner,
        files_task_start(
            owner,
            4,
            "",
            source.join("file.txt").to_str().unwrap(),
            &uid,
            "/rollback.txt",
            2,
        )
        .unwrap(),
    );
    assert_eq!(rollback.state, 3);
    assert!(
        rollback.error.contains("original restored"),
        "{}",
        rollback.error
    );
    assert_eq!(
        std::fs::read(root.join("files/rollback.txt")).unwrap(),
        b"keep original"
    );
    for directory in [root.join("files"), root.join("files/uploaded")] {
        assert!(std::fs::read_dir(directory).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tideterm-")));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let editor = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&prepared.path)
            .unwrap();
        assert!(super::remote_edit::files_edit_close(owner, prepared.edit, true).is_err());
        assert!(std::path::Path::new(&prepared.path).exists());
        drop(editor);
    }
    super::remote_edit::files_edit_close(owner, prepared.edit, true).unwrap();
    assert!(!std::path::Path::new(&prepared.path).exists());
    for path in ["/uploaded", "/new-dir", "/rollback.txt"] {
        let task = wait(
            owner,
            files_task_start(owner, 3, &uid, path, "", "", 0).unwrap(),
        );
        assert_eq!(task.state, 2, "{}", task.error);
    }
    local_files::files_close(owner);
    term::term_close_tab(tab);
    assert!(
        files_task_start(owner, 1, "", "", "", root.join("late").to_str().unwrap(), 0).is_err()
    );
    assert!(!root.join("late").exists());
}
