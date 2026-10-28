use super::*;
use std::sync::atomic::AtomicUsize;

fn snapshot() -> HistorySnapshot {
    let mut grid = tiderm_terminal::TermGrid::new(80, 3);
    grid.feed("SYNTHETIC_HISTORY 中😀\r\nEND".as_bytes());
    grid.history_snapshot().unwrap()
}

#[test]
fn file_commit_failure_and_cancellation_preserve_the_existing_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("history.txt");
    std::fs::write(&target, b"original").unwrap();
    let cancel = Control::default();
    let progress = AtomicUsize::new(0);
    assert!(write(
        snapshot(),
        target.to_str().unwrap(),
        4,
        false,
        &cancel,
        &progress
    )
    .is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    cancel.cancelled.store(true, Ordering::Release);
    assert!(write(
        snapshot(),
        target.to_str().unwrap(),
        1024,
        false,
        &cancel,
        &progress
    )
    .is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        1,
        "partial files must be removed"
    );
    cancel.cancelled.store(false, Ordering::Release);
    assert!(matches!(
        write(
            snapshot(),
            target.to_str().unwrap(),
            1024,
            false,
            &cancel,
            &progress
        )
        .unwrap(),
        Event::Saved
    ));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap().trim_end(),
        "SYNTHETIC_HISTORY 中😀\nEND"
    );
}

#[test]
fn private_large_file_pages_align_utf8_and_unmap_before_cleanup() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let prefix = "a".repeat(super::super::history_file::PAGE_BYTES - 1);
    file.write_all(prefix.as_bytes()).unwrap();
    file.write_all("中😀TAIL".as_bytes()).unwrap();
    let bytes = (200 << 20) + 4;
    file.as_file().set_len(bytes).unwrap();
    let path = file.path().to_path_buf();
    let mut history = HistoryFile::new(file, bytes, true).unwrap();
    assert!(history.mapped());
    assert_eq!(history.page(0).unwrap(), format!("{prefix}中"));
    assert!(history.page(1).unwrap().starts_with("😀TAIL"));
    let file = history.into_file();
    let mut stream = HistoryFile::new(file, bytes, false).unwrap();
    assert!(!stream.mapped());
    assert_eq!(stream.page(0).unwrap(), format!("{prefix}中"));
    assert!(stream.page(1).unwrap().starts_with("😀TAIL"));
    drop(stream);
    assert!(
        !path.exists(),
        "all mappings and handles must retire before unlink"
    );
}

#[test]
fn bridge_job_retires_on_lock_generation_change_and_source_close() {
    use crate::rust::{term, term_diag, term_ops, term_tests};
    let _serial = term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"SYNTHETIC_BRIDGE_HISTORY");
    assert!(history_export_start(tab, "", 4, false).is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while history_export_poll(tab).state == 1 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(history_export_poll(tab).state, 2);
    assert!(history_export_page(tab, 0)
        .text
        .contains("SYNTHETIC_BRIDGE_HISTORY"));
    let path = term::with_tab(tab, |st| {
        st.history_export
            .as_ref()
            .unwrap()
            .file
            .as_ref()
            .unwrap()
            .file
            .path()
            .to_path_buf()
    });
    term_ops::term_set_locked(true);
    assert_eq!(history_export_poll(tab).state, 0);
    term_ops::term_set_locked(false);
    assert!(!path.exists());
    assert!(history_export_start(tab, "", 4, false).is_empty());
    term::with_tab(tab, |st| st.generation += 1);
    assert_eq!(history_export_poll(tab).state, 0);
    term::term_close_tab(tab);
    assert_eq!(history_export_poll(tab).state, 0);
    assert!(!history_export_page(tab, 0).available);
}
