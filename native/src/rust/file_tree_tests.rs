use super::*;

#[tokio::test]
async fn recursive_copy_preserves_bytes_empty_directories_and_conflict_choices() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source");
    let dst = dir.path().join("destination");
    std::fs::create_dir_all(src.join("nested/empty")).unwrap();
    std::fs::write(src.join("first.txt"), b"original").unwrap();
    std::fs::write(src.join("nested/中文.bin"), [0, 255, 17, 42]).unwrap();
    let from = src.to_str().unwrap();
    let to = dst.to_str().unwrap();
    copy(
        &Endpoint::Local,
        from,
        &Endpoint::Local,
        to,
        0,
        &Progress::default(),
    )
    .await
    .unwrap();
    assert!(dst.join("nested/empty").is_dir());
    assert_eq!(
        std::fs::read(dst.join("nested/中文.bin")).unwrap(),
        [0, 255, 17, 42]
    );
    std::fs::write(src.join("first.txt"), b"updated").unwrap();
    assert!(copy(
        &Endpoint::Local,
        from,
        &Endpoint::Local,
        to,
        0,
        &Progress::default()
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(dst.join("first.txt")).unwrap(), b"original");
    copy(
        &Endpoint::Local,
        from,
        &Endpoint::Local,
        to,
        1,
        &Progress::default(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(dst.join("first.txt")).unwrap(), b"original");
    copy(
        &Endpoint::Local,
        from,
        &Endpoint::Local,
        to,
        2,
        &Progress::default(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(dst.join("first.txt")).unwrap(), b"updated");
    remove(&Endpoint::Local, to, &Progress::default())
        .await
        .unwrap();
    assert!(!dst.exists());
    assert!(src.join("first.txt").exists());
}

#[tokio::test]
async fn cancellation_and_containment_reject_before_modifying_destination() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("keep"), b"safe").unwrap();
    let cancelled = Progress::default();
    cancelled.cancel.store(true, Ordering::Relaxed);
    let dst = dir.path().join("destination");
    assert!(copy(
        &Endpoint::Local,
        src.to_str().unwrap(),
        &Endpoint::Local,
        dst.to_str().unwrap(),
        0,
        &cancelled
    )
    .await
    .is_err());
    assert!(!dst.exists());
    assert!(copy(
        &Endpoint::Local,
        src.to_str().unwrap(),
        &Endpoint::Local,
        src.join("child").to_str().unwrap(),
        0,
        &Progress::default()
    )
    .await
    .is_err());
    assert!(!src.join("child").exists());
    assert!(remove(&Endpoint::Local, src.to_str().unwrap(), &cancelled)
        .await
        .is_err());
    assert_eq!(std::fs::read(src.join("keep")).unwrap(), b"safe");
    assert!(validate_path(
        &Endpoint::Local,
        dir.path().join("../escape").to_str().unwrap()
    )
    .is_err());
    let existing = dir.path().join("existing");
    std::fs::write(&existing, b"existing data").unwrap();
    assert!(Endpoint::Local
        .rename_new(
            src.join("keep").to_str().unwrap(),
            existing.to_str().unwrap()
        )
        .await
        .is_err());
    assert_eq!(std::fs::read(existing).unwrap(), b"existing data");
    assert!(src.join("keep").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn recursive_operations_do_not_follow_links_outside_selected_root() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&src).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"safe").unwrap();
    std::os::unix::fs::symlink(&outside, src.join("link")).unwrap();
    let dst = dir.path().join("destination");
    assert!(copy(
        &Endpoint::Local,
        src.to_str().unwrap(),
        &Endpoint::Local,
        dst.to_str().unwrap(),
        0,
        &Progress::default()
    )
    .await
    .is_err());
    assert!(!dst.exists());
    remove(
        &Endpoint::Local,
        src.to_str().unwrap(),
        &Progress::default(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"safe");
}
