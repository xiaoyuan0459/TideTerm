//! The bridge owns pane isolation and lock/retirement admission; matching is tested in the engine.
use super::{term, term_diag, term_marks::*, term_ops, term_tests};
#[test]
fn mark_bridge_isolates_panes_and_rejects_locked_or_retired_reads_writes_and_default_io() {
    let _guard = term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    let other = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"ERROR ERROR\r\n");
    let mark = term_mark_put(tab, 0, "ERROR", 1, 2, true);
    assert_eq!(mark.error, 0);
    assert!(!term_mark_rects(tab).is_empty());
    assert!(term_mark_rows(other).is_empty());
    term_ops::term_set_locked(true);
    let hidden = term_mark_rows(tab).is_empty() && term_mark_rects(tab).is_empty();
    let denied = [
        term_mark_put(tab, mark.id, "WARN", 1, 0, true).error,
        term_mark_remove(tab, mark.id).error,
        term_mark_clear(tab).error,
        term_mark_move(tab, mark.id, true).error,
        term_mark_quick(tab, 0, false).error,
        term_mark_step(tab, mark.id, true).error,
        term_mark_defaults(tab, true).error,
        term_mark_defaults(tab, false).error,
        term_find_selection(tab, true).error,
        term_mark_seed(tab, false).error,
    ];
    term_ops::term_set_locked(false);
    assert!(hidden && denied.iter().all(|&code| code == 6));
    let rows = term_mark_rows(tab);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].pattern, "ERROR");
    assert_eq!(rows[0].color, 2);
    assert!(rows[0].enabled);
    term::term_close_tab(tab);
    assert!(term_mark_rows(tab).is_empty() && term_mark_rects(tab).is_empty());
    assert_eq!(term_mark_put(tab, mark.id, "WARN", 1, 0, true).error, 7);
    assert_eq!(term_mark_remove(tab, mark.id).error, 7);
    assert_eq!(term_mark_clear(tab).error, 7);
    assert_eq!(term_mark_defaults(tab, true).error, 7);
    assert_eq!(term_mark_seed(tab, false).error, 7);
    assert!(term_mark_rows(other).is_empty());
    term::term_close_tab(other);
}
