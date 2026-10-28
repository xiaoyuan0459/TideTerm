//! Public pane contracts for retained selections; no transport or clipboard fixtures.
use tiderm_terminal::{pane::reading::ReadAction, SoftwareRenderer, TerminalPane};

const W: f32 = 200.0;
const H: f32 = 80.0;

fn pane() -> TerminalPane {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    p.feed("0123456789\r\nABCDEFGHIJ\r\nklmnopqrst");
    p
}

fn at(col: usize, row: usize) -> (f32, f32) {
    (col as f32 * 10.0 + 5.0, row as f32 * 20.0 + 10.0)
}

fn drag(p: &mut TerminalPane, start: (usize, usize), end: (usize, usize), add: bool, column: bool) {
    let (x, y) = at(start.0, start.1);
    if add {
        assert!(p.selection_begin_add(x, y, W, H, column));
    } else if column {
        p.selection_begin_column(x, y, W, H);
    } else {
        p.selection_begin(x, y, W, H);
    }
    let (x, y) = at(end.0, end.1);
    p.selection_update(x, y, W, H);
    p.end_selection_drag();
}

#[test]
fn mixed_ranges_copy_in_source_order_and_edits_only_change_the_active_id() {
    let mut p = pane();
    drag(&mut p, (1, 2), (3, 2), false, false);
    let first = p.selection_status().0;
    drag(&mut p, (2, 0), (4, 1), true, true);
    let second = p.selection_status().0;
    assert_ne!(first, second);
    assert_eq!(p.selection_status(), (second, 2, 2));
    assert_eq!(p.selection_text().as_deref(), Some("234\nCDE\nlmn"));
    assert_eq!(p.selection_rects(W, H).len(), 3);
    assert_eq!(p.selection_active_rects(W, H).len(), 2);

    assert!(p.selection_cycle(true));
    assert_eq!(p.selection_status(), (first, 1, 2));
    assert!(p.selection_extend_to(45.0, 50.0, W, H, false));
    assert_eq!(p.selection_text().as_deref(), Some("234\nCDE\nlmno"));
    assert!(p.selection_cycle(false));
    assert_eq!(p.selection_status().0, second);
    assert!(p.selection_swap_anchor());
    assert_eq!(p.selection_text().as_deref(), Some("234\nCDE\nlmno"));
    assert!(
        !p.column_selection(),
        "cycling a rectangle keeps the next drag's policy"
    );
    assert!(p.selection_remove_active());
    assert_eq!(p.selection_status(), (first, 1, 1));
    assert_eq!(p.selection_text().as_deref(), Some("lmno"));
    assert!(!p.selection_cycle(true));
}

#[test]
fn append_click_preserves_ranges_and_a_normal_drag_replaces_all() {
    let mut p = pane();
    drag(&mut p, (0, 0), (2, 0), false, false);
    let first = p.selection_status();
    assert!(p.selection_begin_add(65.0, 10.0, W, H, false));
    p.selection_update(67.0, 10.0, W, H);
    p.end_selection_drag();
    assert_eq!(p.selection_status(), first);
    assert_eq!(p.selection_text().as_deref(), Some("012"));
    drag(&mut p, (1, 0), (3, 0), true, false);
    assert_eq!(
        p.selection_text().as_deref(),
        Some("012\n123"),
        "overlap retains distinct ranges"
    );
    assert!(
        p.type_over_selection().is_none(),
        "multiple highlights must not erase shell input"
    );
    drag(&mut p, (2, 1), (4, 1), false, false);
    assert_eq!(p.selection_status().2, 1);
    assert_eq!(p.selection_text().as_deref(), Some("CDE"));
    assert_eq!(p.selection_rects(W, H).len(), 1);
    p.selection_clear();
    assert_eq!(p.selection_status(), (0, 0, 0));
    assert!(p.selection_rects(W, H).is_empty());
}

#[test]
fn incoming_output_reflow_and_screen_transitions_keep_or_retire_each_range() {
    let mut p = pane();
    drag(&mut p, (0, 0), (3, 0), false, false);
    let stream = p.selection_status().0;
    drag(&mut p, (2, 1), (4, 2), true, true);
    let selected = p.selection_text();
    p.feed("\r\nnew row\r\nanother row");
    assert_eq!(p.selection_text(), selected);
    p.resize(10, 4);
    assert_eq!(p.selection_status(), (stream, 1, 1));
    assert_eq!(p.selection_text().as_deref(), Some("0123"));
    for clear in ["\x1b[?1049h", "\x1b[2J", "\x1b[3J", "\x1bc"] {
        let mut p = pane();
        drag(&mut p, (0, 0), (2, 0), false, false);
        drag(&mut p, (0, 1), (2, 1), true, false);
        p.feed(clear);
        assert_eq!(p.selection_count(), 0, "{clear:?}");
        assert_eq!(p.selection_text(), None);
        assert!(p.selection_rects(W, H).is_empty());
    }
}

#[test]
fn history_eviction_prunes_old_ranges_without_retargeting_the_survivor() {
    // Keep one compressed row: a zero-byte store must now retire even its newest row.
    let mut p = TerminalPane::with_history(20, 4, Box::new(SoftwareRenderer::new()), 4, 200);
    p.feed("old0\r\nold1\r\nold2\r\nnew3");
    drag(&mut p, (0, 0), (3, 0), false, false);
    drag(&mut p, (0, 3), (3, 3), true, false);
    let survivor = p.selection_status().0;
    p.feed("\r\n4\r\n5\r\n6\r\n7\r\n8");
    assert_eq!(p.selection_status(), (survivor, 1, 1));
    assert_eq!(p.selection_text().as_deref(), Some("new3"));
    p.feed("\r\n9\r\n10\r\n11\r\n12\r\n13\r\n14");
    assert_eq!(p.selection_count(), 0);
    assert_eq!(p.selection_text(), None);
}

#[test]
fn reading_tab_edits_and_register_copy_use_the_same_retained_ranges() {
    let mut p = pane();
    drag(&mut p, (0, 0), (2, 0), false, false);
    let first = p.selection_status().0;
    drag(&mut p, (0, 1), (2, 1), true, false);
    let second = p.selection_status().0;
    p.set_reading_mode(true);
    p.reading_key(12, 0, "");
    assert_eq!(p.selection_status().0, first);
    p.reading_key(4, 0, "");
    assert_eq!(p.selection_text().as_deref(), Some("0123\nABC"));
    p.reading_key(13, 1, "");
    assert_eq!(p.selection_status().0, second);
    p.reading_key(0, 0, "y");
    match p.reading_key(0, 0, "P") {
        ReadAction::Paste { bytes, stay } => {
            assert_eq!(bytes, b"0123\rABC");
            assert!(stay);
        }
        _ => panic!("yank must populate the reading register"),
    }
    p.reading_key(15, 0, "");
    assert_eq!(p.selection_count(), 0);
    p.set_multi_selection(true);
    p.reading_key(0, 0, "v");
    p.reading_key(4, 0, "");
    p.reading_key(0, 0, "v"); // finish this visual range
    p.reading_key(2, 0, "");
    p.reading_key(0, 0, "v"); // append a keyboard range
    assert_eq!(p.selection_count(), 2);
    let other = pane();
    assert!(!other.multi_selection());
    assert_eq!(other.selection_count(), 0);
}

#[test]
fn count_and_aggregate_copy_budgets_reject_without_partial_text_or_lost_ranges() {
    let mut p = TerminalPane::new(1024, 50, Box::new(SoftwareRenderer::new()));
    p.feed(&vec!["x".repeat(1024); 50].join("\r\n"));
    p.select_all();
    for _ in 1..32 {
        assert!(p.selection_begin_add(5.0, 10.0, 10240.0, 1000.0, false));
        p.selection_update(10235.0, 990.0, 10240.0, 1000.0);
        p.end_selection_drag();
    }
    let before = p.selection_status();
    assert!(!p.selection_begin_add(5.0, 10.0, 10240.0, 1000.0, true));
    assert_eq!(p.selection_status(), before);
    assert!(p.selection_text_result().is_err());
    assert_eq!(
        p.selection_text(),
        None,
        "budget failure must not return a partial clipboard"
    );
    assert_eq!(p.selection_count(), 32);
    while p.selection_count() > 1 {
        assert!(p.selection_remove_active());
    }
    assert_eq!(p.selection_text().unwrap().len(), 51249);
}
