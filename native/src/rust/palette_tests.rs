use super::{palette::*, store, term, term_diag, term_ops, term_reading, term_tests};

#[test]
fn line_bridge_rejects_generation_lock_and_retirement_without_changing_mode_or_uid() {
    let _serial = term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"ONE\r\nTWO\r\nTHREE\r\n");
    let uid = term::term_uid(tab);
    let state = term_line_begin(tab, 0);
    assert_eq!(state.error, 0);
    let target = term_line_preview(tab, 0, state.context, "2");
    assert_eq!(target.text, "TWO");
    assert!(!term_line_commit(tab, 1, state.context, target.token));
    assert!(!term_reading::term_reading(tab));
    term_ops::term_set_locked(true);
    let begin = term_line_begin(tab, 0);
    let preview = term_line_preview(tab, 0, state.context, "1");
    let attempted = term_line_commit(tab, 0, state.context, target.token);
    term_ops::term_set_locked(false);
    assert_eq!(begin.error, 5);
    assert_eq!(preview.error, 5);
    assert!(!attempted);
    assert!(term_line_commit(tab, 0, state.context, target.token));
    assert!(term_reading::term_reading(tab));
    assert_eq!(term::term_uid(tab), uid);
    assert!(!term_line_commit(tab, 0, state.context, target.token));
    term::term_close_tab(tab);
    assert_eq!(term_line_begin(tab, 0).error, 6);
    assert!(!term_line_commit(tab, 0, state.context, target.token));
    term_line_end(tab, state.context);
}

struct StoreScope;
impl Drop for StoreScope {
    fn drop(&mut self) {
        store::set_store_dir(None);
        term_ops::term_set_locked(false);
    }
}

#[test]
fn session_catalog_is_scoped_and_deduplicated_and_saved_open_rechecks_lock_at_the_rust_boundary() {
    let _serial = term_tests::input_lock_guard();
    let dir = tempfile::tempdir().unwrap();
    store::set_store_dir(Some(dir.path().into()));
    let _scope = StoreScope;
    let mut cfg = store::SessionConfig::blank(store::SessionKind::Local);
    cfg.name = "Palette 中 fixture".into();
    cfg.meta.group_path = "Test > Palette".into();
    if let Some(local) = &mut cfg.local {
        local.shell = if cfg!(windows) { "pwsh" } else { "sh" }.into();
        local.args = if cfg!(windows) {
            "-NoProfile -NonInteractive"
        } else {
            ""
        }
        .into();
        local.working_directory = dir.path().to_string_lossy().into();
    }
    cfg.id = store::save_returning_id(&cfg).unwrap();
    let tab = term::open_tab_with_config(&cfg);
    let other = term::term_new_tab();
    let rows = palette_sessions(&[tab, tab]);
    assert_eq!(rows.error, 0);
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0].tab, tab);
    assert_eq!(rows.rows[0].saved, cfg.id);
    assert!(rows.rows[0].title.contains("Palette"));
    assert_eq!(rows.rows[0].group, "Test > Palette");
    let positive = crate::bridge::session::sess_open(&cfg.id);
    assert_ne!(positive, u32::MAX);
    term::term_close_tab(positive);
    term_ops::term_set_locked(true);
    let hidden = palette_sessions(&[tab]);
    let exists = palette_saved_exists(&cfg.id);
    let denied = crate::bridge::session::sess_open(&cfg.id);
    term_ops::term_set_locked(false);
    if denied != u32::MAX {
        term::term_close_tab(denied);
    }
    assert!(hidden.rows.is_empty() && hidden.error == 5 && !exists);
    assert_eq!(
        denied,
        u32::MAX,
        "saved sessions must not start a transport while locked"
    );
    let file = dir.path().join("sessions.json");
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let unreadable = palette_sessions(&[other]);
    assert_eq!(unreadable.error, 8);
    assert_eq!(unreadable.rows.len(), 1);
    assert_eq!(unreadable.rows[0].tab, other);
    assert!(!palette_saved_exists(&cfg.id));
    assert_eq!(crate::bridge::session::sess_open(&cfg.id), u32::MAX);
    term::term_close_tab(tab);
    term::term_close_tab(other);
}
