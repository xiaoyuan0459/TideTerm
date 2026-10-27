//! Observable filesystem metadata and platform filename contracts.
use crate::session_options::SessionOptions;
use crate::sftp::SftpFileStat;
use crate::sftp_policy::{local_file_name, preserve_download_times};
use std::time::{Duration, UNIX_EPOCH};

#[tokio::test]
async fn downloaded_file_preserves_selected_times_without_changing_contents() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("transfer.bin");
    std::fs::write(&file, b"complete payload").unwrap();
    let accessed = UNIX_EPOCH + Duration::from_secs(1_600_000_001);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_accessed(accessed))
        .unwrap();
    let remote = SftpFileStat {
        size_bytes: 16,
        modified_unix: 1_600_000_010,
        accessed_unix: Some(1_600_000_020),
        is_dir: false,
        permissions: 0o600,
        owner: None,
        group: None,
    };
    let mut options = SessionOptions::default();
    preserve_download_times(&file, &remote, &options)
        .await
        .unwrap();
    let metadata = std::fs::metadata(&file).unwrap();
    assert_eq!(
        metadata.modified().unwrap(),
        UNIX_EPOCH + Duration::from_secs(1_600_000_010)
    );
    assert_eq!(metadata.accessed().unwrap(), accessed);
    options.sftp_preserve_accessed = true;
    preserve_download_times(&file, &remote, &options)
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().accessed().unwrap(),
        UNIX_EPOCH + Duration::from_secs(1_600_000_020)
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"complete payload");
}

#[test]
fn remote_names_never_escape_the_download_directory() {
    for name in ["", ".", "..", "../file", "a/b", "nul\0"] {
        assert!(local_file_name(name, true).is_err());
    }
    assert_eq!(
        local_file_name("普通文件.txt", true).unwrap(),
        "普通文件.txt"
    );
    #[cfg(windows)]
    for (remote, local) in [
        ("CON.txt", "_CON.txt"),
        ("a\\b:?.txt ", "a_b__.txt"),
        ("LPT1.log", "_LPT1.log"),
    ] {
        assert!(local_file_name(remote, false).is_err());
        assert_eq!(local_file_name(remote, true).unwrap(), local);
    }
}
