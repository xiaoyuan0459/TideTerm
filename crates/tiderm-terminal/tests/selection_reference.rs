//! Selection-extraction reference tests — drive [`TerminalPane`]'s drag-selection API with
//! logical-pixel points (the way the Slint shell does) and assert the extracted text.
//!
//! Headless: panes use the [`SoftwareRenderer`] but never render — `selection_text` works
//! straight off the grid snapshot. Geometry: a pane of C×R cells over a `surf_w`×`surf_h`
//! surface has cells of `surf_w/C` × `surf_h/R` logical px (see `TerminalPane::cell_logical`),
//! so with a 10×20 px cell, the point (5, 10) lands in cell (0, 0).
//!
//! The drag threshold (4 px) is part of the contract: a press + sub-threshold twitch must
//! NOT become a selection (that's the clipboard-clobber guard), so tests drag well past it.

use tiderm_terminal::{SoftwareRenderer, TerminalPane};

/// A 20×4 pane over a 200×80 surface → exact 10×20 px cells.
const SURF_W: f32 = 200.0;
const SURF_H: f32 = 80.0;

fn pane_with(content: &str) -> TerminalPane {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    p.feed(content);
    p
}

/// Center of cell (col, row) in logical px for the 10×20 cell geometry.
fn at(col: usize, row: usize) -> (f32, f32) {
    (col as f32 * 10.0 + 5.0, row as f32 * 20.0 + 10.0)
}

fn drag(p: &mut TerminalPane, from: (usize, usize), to: (usize, usize)) {
    let (x0, y0) = at(from.0, from.1);
    let (x1, y1) = at(to.0, to.1);
    p.selection_begin(x0, y0, SURF_W, SURF_H);
    p.selection_update(x1, y1, SURF_W, SURF_H);
}

/// Re-aim a live selection at a cell of the shared 20×4 fixture geometry.
fn extend(p: &mut TerminalPane, col: usize, row: usize) -> bool {
    let (x, y) = at(col, row);
    p.selection_extend_to(x, y, SURF_W, SURF_H, false)
}

#[test]
fn single_row_drag_extracts_the_inclusive_cell_range() {
    let mut p = pane_with("hello world");
    drag(&mut p, (0, 0), (7, 0));
    assert!(p.selection_is_drag());
    assert_eq!(p.selection_text().as_deref(), Some("hello wo"));
}

#[test]
fn multi_row_drag_joins_rows_with_newlines_and_trims_trailing_blanks() {
    let mut p = pane_with("hello world\r\nsecond");
    drag(&mut p, (0, 0), (3, 1));
    // Row 0 runs from the anchor to the line end (trailing blanks trimmed),
    // row 1 from column 0 to the head cell, inclusive.
    assert_eq!(p.selection_text().as_deref(), Some("hello world\nseco"));
}

#[test]
fn reverse_drag_reads_in_document_order() {
    let mut p = pane_with("hello world\r\nsecond");
    drag(&mut p, (3, 1), (0, 0)); // drag up-left: head before anchor
    assert_eq!(p.selection_text().as_deref(), Some("hello world\nseco"));
}

#[test]
fn a_click_without_a_real_drag_selects_nothing() {
    let mut p = pane_with("hello");
    let (x, y) = at(2, 0);
    p.selection_begin(x, y, SURF_W, SURF_H);
    // A 2-px twitch is inside the drag threshold — even across a cell boundary it must
    // not become a selection (the copy-on-select clipboard-clobber guard).
    p.selection_update(x + 2.0, y, SURF_W, SURF_H);
    assert!(!p.selection_is_drag());
    assert_eq!(p.selection_text(), None);
}

#[test]
fn drag_past_the_grid_edge_clamps_to_the_border_cell() {
    let mut p = pane_with("0123456789abcdefghij");
    let (x0, y0) = at(15, 0);
    p.selection_begin(x0, y0, SURF_W, SURF_H);
    // Stray well past the right/bottom edge — the head clamps to the last cell of row 0
    // when y stays on row 0's band... and to the grid corner when both overshoot.
    p.selection_update(SURF_W + 50.0, y0, SURF_W, SURF_H);
    assert_eq!(p.selection_text().as_deref(), Some("fghij"));
}

#[test]
fn select_all_covers_the_visible_screen() {
    let mut p = pane_with("first\r\nsecond\r\nthird");
    p.select_all();
    // The 4-row screen has one trailing blank row — select-all keeps it as a final
    // newline (each row is right-trimmed, rows joined by `\n`).
    assert_eq!(
        p.selection_text().as_deref(),
        Some("first\nsecond\nthird\n")
    );
}

#[test]
fn selection_over_blank_cells_yields_none() {
    let mut p = pane_with(""); // empty grid
    drag(&mut p, (0, 0), (5, 0));
    assert_eq!(
        p.selection_text(),
        None,
        "an all-blank row extracts no text"
    );
}

#[test]
fn clearing_the_selection_drops_the_text() {
    let mut p = pane_with("hello");
    drag(&mut p, (0, 0), (4, 0));
    assert_eq!(p.selection_text().as_deref(), Some("hello"));
    p.selection_clear();
    assert_eq!(p.selection_text(), None);
}

#[test]
fn column_drag_and_mode_conversion_copy_fixed_columns_in_every_direction() {
    for (from, to) in [
        ((1, 0), (3, 1)),
        ((3, 1), (1, 0)),
        ((3, 0), (1, 1)),
        ((1, 1), (3, 0)),
    ] {
        let mut p = pane_with("abcdef\r\nABCDEF");
        p.set_column_selection(true);
        drag(&mut p, from, to);
        assert_eq!(p.selection_text().as_deref(), Some("bcd\nBCD"));
        assert_eq!(
            p.selection_rects(SURF_W, SURF_H),
            vec![(10.0, 0.0, 30.0, 20.0), (10.0, 20.0, 30.0, 20.0)]
        );
        assert_eq!(p.type_over_selection(), None);
    }
    let mut p = pane_with("abcdef\r\nAB");
    drag(&mut p, (1, 0), (3, 1));
    p.set_column_selection(true);
    assert_eq!(p.selection_text().as_deref(), Some("bcd\nB  "));
    p.set_column_selection(false);
    assert_eq!(p.selection_text().as_deref(), Some("bcdef\nAB"));
}

#[test]
fn columns_keep_softwrap_rows_and_complete_wide_combining_glyphs() {
    let mut p = pane_with("01234567890123456789ABCDEFGHIJKLMNOPQRST");
    p.set_column_selection(true);
    drag(&mut p, (2, 0), (5, 1));
    assert_eq!(p.selection_text().as_deref(), Some("2345\nCDEF"));
    let mut p = pane_with("A中e\u{301}Z\r\nAbcdZ");
    p.set_column_selection(true);
    drag(&mut p, (2, 0), (2, 1));
    assert_eq!(p.selection_text().as_deref(), Some("中\nc"));
    assert_eq!(
        p.selection_rects(SURF_W, SURF_H),
        vec![(10.0, 0.0, 20.0, 20.0), (20.0, 20.0, 10.0, 20.0)]
    );
    drag(&mut p, (3, 0), (3, 1));
    assert_eq!(p.selection_text().as_deref(), Some("e\u{301}\nd"));
}

#[test]
fn column_selection_follows_compressed_history_and_retires_on_width_change() {
    let mut p = TerminalPane::with_history(20, 4, Box::new(SoftwareRenderer::new()), 2, 1_000_000);
    p.feed("A中e\u{301}Z\r\nAbcdZ\r\n");
    for _ in 0..25 {
        p.feed("later output\r\n");
    }
    p.scroll_to_top();
    p.set_column_selection(true);
    drag(&mut p, (2, 0), (3, 1));
    assert_eq!(p.selection_text().as_deref(), Some("中e\u{301}\ncd"));
    p.end_selection_drag();
    p.feed("more output\r\n");
    assert_eq!(p.selection_text().as_deref(), Some("中e\u{301}\ncd"));
    p.resize(20, 6);
    assert_eq!(p.selection_text().as_deref(), Some("中e\u{301}\ncd"));
    p.resize(12, 6);
    assert_eq!(p.selection_text(), None);
    assert!(p.column_selection());
    p.set_column_selection(false);
    p.selection_begin_column(5.0, 10.0, 120.0, 120.0);
    p.selection_update(25.0, 30.0, 120.0, 120.0);
    assert!(!p.column_selection()); // Alt-style gesture is temporary.
    p.feed("\x1b[?1049hTUI");
    assert_eq!(p.selection_text(), None);
    p.feed("\x1b[?1049l");
    p.clear_scrollback();
    assert_eq!(p.selection_text(), None);
}

#[test]
fn column_copy_budget_rejects_large_padding_without_partial_copy() {
    let mut p =
        TerminalPane::with_history(160, 4, Box::new(SoftwareRenderer::new()), 2, 32_000_000);
    p.feed(&"x\r\n".repeat(9000));
    p.scroll_to_top();
    p.set_column_selection(true);
    assert!(p.total_history() > 7000);
    p.selection_begin(5.0, 10.0, 1600.0, 80.0);
    p.scroll_to_bottom();
    p.selection_update(1595.0, 70.0, 1600.0, 80.0);
    assert!(p.selection_is_drag());
    assert!(p.selection_text().is_none());
    assert_eq!(p.copy_selection(), None);
    assert!(p.toast_text().unwrap().contains("1 MiB"));
}

#[test]
fn column_mode_and_temporary_drag_expand_folds_before_hit_testing() {
    let mut p = TerminalPane::new(20, 8, Box::new(SoftwareRenderer::new()));
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo ONE\r\n\x1b]133;C\x07AAA\r\nBBB\r\nCCC\r\n\x1b]133;D\x07");
    let id = p.command_blocks()[0].id;
    p.command_set_folded(id, true);
    assert!(!p.screen_text().contains("BBB"));
    p.set_column_selection(true);
    assert!(p.screen_text().contains("BBB"));
    p.selection_begin(5.0, 30.0, 200.0, 160.0);
    p.selection_update(25.0, 70.0, 200.0, 160.0);
    assert_eq!(p.selection_text().as_deref(), Some("AAA\nBBB\nCCC"));
    p.set_column_selection(false);
    p.command_set_folded(id, true);
    p.selection_begin_column(5.0, 30.0, 200.0, 160.0);
    p.selection_update(25.0, 70.0, 200.0, 160.0);
    assert!(!p.column_selection());
    assert!(!p.command_blocks()[0].folded);
    assert_eq!(p.selection_text().as_deref(), Some("AAA\nBBB\nCCC"));
}

#[test]
fn select_to_keeps_the_anchor_and_extends_both_ways() {
    let mut p = pane_with("0123456789\r\nABCDEFGHIJ\r\n");
    // No selection yet: the gesture reports failure so the view can start a fresh drag.
    assert!(!extend(&mut p, 4, 0));
    drag(&mut p, (2, 0), (2, 0));
    p.end_selection_drag();
    assert!(
        !p.has_selection(),
        "a press without movement is not a selection"
    );

    drag(&mut p, (2, 0), (4, 0));
    p.end_selection_drag();
    assert_eq!(p.selection_text().as_deref(), Some("234"));
    assert!(extend(&mut p, 6, 1));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("23456789\nABCDEFG"),
        "the anchor stays put and the head runs onto the next row"
    );
    // Aiming back above the anchor keeps it: the head simply moves, and the stream is still read
    // in reading order between the two ends.
    assert!(extend(&mut p, 1, 0));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("12"),
        "a head above the anchor re-reads the same stream in reading order"
    );
}

#[test]
fn select_to_covers_whole_wide_glyphs_wherever_it_lands() {
    let mut p = pane_with("A中B\r\n");
    drag(&mut p, (0, 0), (1, 0));
    p.end_selection_drag();
    assert_eq!(p.selection_text().as_deref(), Some("A中"));
    // The head re-aims at an exact cell, with no press in flight: an explicit select-to is not
    // subject to the drag dead zone that keeps a stray click twitch from copying.
    assert!(p.selection_extend_to(32.0, 10.0, SURF_W, SURF_H, false));
    assert_eq!(p.selection_text().as_deref(), Some("A中B"));
    // Aiming back at the leading half of the wide glyph keeps covering the whole glyph.
    assert!(extend(&mut p, 1, 0));
    assert_eq!(p.selection_text().as_deref(), Some("A中"));
    assert!(extend(&mut p, 3, 0));
    assert_eq!(p.selection_text().as_deref(), Some("A中B"));
}

#[test]
fn select_to_follows_the_current_stream_or_column_mode() {
    let mut p = pane_with("0123456789\r\nABCDEFGHIJ\r\n");
    drag(&mut p, (2, 0), (4, 0));
    p.end_selection_drag();
    // Column mode re-aims the LIVE selection as a rectangle between the same two endpoints —
    // no new drag is needed, and the anchor keeps column 2.
    p.set_column_selection(true);
    assert_eq!(p.selection_text().as_deref(), Some("234"));
    assert!(extend(&mut p, 6, 1));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("23456\nCDEFG"),
        "the head widens the rectangle from its own column"
    );
    p.set_column_selection(false);
    assert!(extend(&mut p, 4, 1));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("23456789\nABCDE"),
        "switching back re-reads the same endpoints as a stream"
    );
}

#[test]
fn swap_anchor_keeps_the_text_and_moves_the_growing_end() {
    let mut p = pane_with("0123456789\r\nABCDEFGHIJ\r\n");
    drag(&mut p, (2, 0), (4, 0));
    p.end_selection_drag();
    let before = p.selection_text().unwrap();
    assert!(p.selection_swap_anchor());
    assert_eq!(
        p.selection_text().unwrap(),
        before,
        "the covered text is unchanged"
    );
    assert!(extend(&mut p, 7, 0));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("4567"),
        "after the swap the selection grows from the opposite end"
    );
    assert!(p.selection_swap_anchor());
    assert_eq!(
        p.selection_text().as_deref(),
        Some("4567"),
        "swapping back flips the direction without moving the range"
    );
    assert!(extend(&mut p, 1, 0));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("1234567"),
        "the second swap really did re-anchor at the other end"
    );

    // Both persistent mode and the Alt-style temporary gesture must retain their shape.
    for persistent_column in [true, false] {
        let mut p = pane_with("0123456789\r\nABCDEFGHIJ\r\n");
        p.set_column_selection(persistent_column);
        p.selection_begin_column(25.0, 10.0, SURF_W, SURF_H);
        p.selection_update(45.0, 30.0, SURF_W, SURF_H);
        p.end_selection_drag();
        assert_eq!(p.selection_text().as_deref(), Some("234\nCDE"));
        assert!(p.selection_swap_anchor());
        assert_eq!(p.selection_text().as_deref(), Some("234\nCDE"));
        assert_eq!(
            p.selection_rects(SURF_W, SURF_H),
            vec![(20.0, 0.0, 30.0, 20.0), (20.0, 20.0, 30.0, 20.0)]
        );
        assert_eq!(p.column_selection(), persistent_column);
        assert!(extend(&mut p, 6, 0));
        assert_eq!(p.selection_text().as_deref(), Some("456\nEFG"));
        assert_eq!(p.column_selection(), persistent_column);
    }
}

#[test]
fn swap_anchor_is_rejected_without_a_selection() {
    let mut p = pane_with("0123456789\r\n");
    assert!(!p.has_selection());
    assert!(!p.selection_swap_anchor());
    // A stationary press is not yet a selection, so it must not be swappable either.
    drag(&mut p, (3, 0), (3, 0));
    p.end_selection_drag();
    assert!(!p.has_selection());
    assert!(!p.selection_swap_anchor());
    p.selection_clear();
    assert!(!extend(&mut p, 5, 0));
}

#[test]
fn select_to_reveals_folded_output_before_aiming() {
    let mut p = TerminalPane::new(20, 8, Box::new(SoftwareRenderer::new()));
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo ONE\r\n\x1b]133;C\x07AAA\r\nBBB\r\nCCC\r\n\x1b]133;D\x07");
    let id = p.command_blocks()[0].id;
    // This pane is 20×8, so it needs its own 200×160 surface for the same 10×20 px cells the
    // shared fixture assumes — otherwise the rows would be half-height and the mapping wrong.
    const W: f32 = 200.0;
    const H: f32 = 160.0;
    let at = |col: usize, row: usize| (col as f32 * 10.0 + 5.0, row as f32 * 20.0 + 10.0);
    // Starting a drag inside the block reveals it (existing selection_begin policy), so the
    // selection below can only be extended onto rows that are already visible.
    let (x0, y0) = at(0, 0);
    let (x1, y1) = at(2, 0);
    p.selection_begin(x0, y0, W, H);
    p.selection_update(x1, y1, W, H);
    p.end_selection_drag();
    assert!(p.screen_text().contains("BBB"));
    assert_eq!(p.selection_text().as_deref(), Some("$ e"));
    let (x2, y2) = at(2, 2);
    assert!(p.selection_extend_to(x2, y2, W, H, false));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("$ echo ONE\nAAA\nBBB"),
        "the head lands on the visible row it was aimed at"
    );

    // Folding is the boundary case: it drops the selection outright, so there is nothing left to
    // re-aim and select-to declines rather than pointing at a hidden row.
    p.command_set_folded(id, true);
    assert!(!p.screen_text().contains("BBB"));
    assert!(!p.has_selection());
    assert!(!p.selection_extend_to(x2, y2, W, H, false));
}
