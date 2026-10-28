//! Observable selection-edit contracts through the public pane, without a transport.
use tiderm_terminal::pane::SelectionEditError;
use tiderm_terminal::{SoftwareRenderer, TerminalPane};

fn pane(cols: usize, rows: usize, text: &str) -> TerminalPane {
    let mut p = TerminalPane::new(cols, rows, Box::new(SoftwareRenderer::new()));
    p.feed(text);
    p
}
fn drag(p: &mut TerminalPane, start: (usize, usize), end: (usize, usize), add: bool) {
    let (cols, rows) = p.grid_size();
    let (w, h) = (cols as f32 * 10.0, rows as f32 * 20.0);
    let point = |c: (usize, usize)| (c.0 as f32 * 10.0 + 5.0, c.1 as f32 * 20.0 + 10.0);
    let (x, y) = point(start);
    if add {
        assert!(p.selection_begin_add(x, y, w, h, false));
    } else {
        p.selection_begin(x, y, w, h);
    }
    let (x, y) = point(end);
    p.selection_update(x, y, w, h);
    p.end_selection_drag();
}

#[test]
fn a_locked_selection_survives_edits_but_never_survives_source_retirement() {
    let mut p = pane(20, 4, "0123456789\r\nABCDEFGHIJ");
    assert!(!p.set_selection_locked(true));
    drag(&mut p, (1, 0), (3, 0), false);
    p.feed("\x1b[1;10H");
    assert!(p.set_selection_locked(true));
    drag(&mut p, (2, 1), (4, 1), false);
    assert_eq!(p.selection_text().as_deref(), Some("123"));
    assert!(!p.selection_extend_to(75.0, 30.0, 200.0, 80.0, false));
    assert_eq!(p.selection_split(), Err(SelectionEditError::Locked));
    assert_eq!(p.selection_invert(), Err(SelectionEditError::Locked));
    assert!(p.type_over_selection().is_none());
    p.set_reading_mode(true);
    p.reading_key(0, 0, "v");
    p.reading_key(0, 0, "j");
    assert_eq!(p.selection_text().as_deref(), Some("123"));
    p.feed("\x1b[2J");
    assert_eq!(p.selection_count(), 0);
    assert!(!p.selection_locked());
}

#[test]
fn split_preserves_text_and_refuses_an_oversized_result_without_partial_edits() {
    let mut p = pane(20, 4, "0123456789\r\nABCDEFGHIJ\r\nklmnopqrst");
    drag(&mut p, (2, 0), (4, 2), false);
    let original = p.selection_status().0;
    p.set_reading_mode(true);
    assert_eq!(p.selection_split(), Ok(()));
    assert_eq!(p.selection_count(), 3);
    assert_eq!(p.selection_status().0, original);
    assert_eq!(
        p.selection_text().as_deref(),
        Some("23456789\nABCDEFGHIJ\nklmno")
    );
    p.reading_key(5, 0, ""); // Home extends only the new active first row.
    assert_eq!(
        p.selection_text().as_deref(),
        Some("012\nABCDEFGHIJ\nklmno")
    );
    assert!(p.selection_remove_active());
    assert_eq!(p.selection_text().as_deref(), Some("ABCDEFGHIJ\nklmno"));
    let mut wrapped = pane(5, 2, "abcdefgh");
    drag(&mut wrapped, (1, 0), (2, 1), false);
    assert_eq!(wrapped.selection_text().as_deref(), Some("bcdefgh"));
    assert_eq!(wrapped.selection_split(), Ok(()));
    assert_eq!(wrapped.selection_text().as_deref(), Some("bcde\nfgh"));
    let mut p = pane(20, 4, "a\r\nb");
    p.selection_begin_column(5.0, 10.0, 200.0, 80.0);
    p.selection_update(35.0, 30.0, 200.0, 80.0);
    p.end_selection_drag();
    assert_eq!(p.selection_split(), Ok(()));
    assert_eq!(p.selection_text().as_deref(), Some("a   \nb   "));
    let mut p = pane(
        20,
        40,
        &(0..40).map(|_| "row").collect::<Vec<_>>().join("\r\n"),
    );
    p.select_all();
    let before = (p.selection_status(), p.selection_text());
    assert_eq!(p.selection_split(), Err(SelectionEditError::Limit));
    assert_eq!((p.selection_status(), p.selection_text()), before);
}

#[test]
fn inversion_uses_whole_visible_glyphs_and_budget_failure_preserves_the_union() {
    let mut p = pane(20, 2, "abc汉e\u{301} def\r\nXYZ");
    drag(&mut p, (4, 0), (5, 0), false);
    assert_eq!(p.selection_invert(), Ok(()));
    assert_eq!(p.selection_text().as_deref(), Some("abc\n def\nXYZ"));
    assert_eq!(p.selection_invert(), Ok(()));
    assert_eq!(p.selection_text().as_deref(), Some("汉e\u{301}"));
    let mut p = pane(130, 1, &"x".repeat(129));
    for n in 0..32 {
        drag(&mut p, (1 + 4 * n, 0), (2 + 4 * n, 0), n > 0);
    }
    let before = (p.selection_status(), p.selection_text());
    assert_eq!(p.selection_invert(), Err(SelectionEditError::Limit));
    assert_eq!((p.selection_status(), p.selection_text()), before);
}

#[test]
fn double_click_can_select_a_path_or_its_component_without_resolving_it() {
    let mut p = pane(40, 2, "C:\\repo\\中国-file.txt");
    assert!(p.selection_token_at(95.0, 10.0, 400.0, 40.0, false));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("C:\\repo\\中国-file.txt")
    );
    assert!(p.selection_token_at(95.0, 10.0, 400.0, 40.0, true));
    assert_eq!(p.selection_text().as_deref(), Some("中国-file.txt"));
}
