use super::*;
use crate::render::SoftwareRenderer;

#[test]
fn feed_then_dirty_then_render_roundtrips() {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    assert!(p.take_dirty()); // starts dirty
    assert!(!p.take_dirty());
    p.feed("hi");
    assert!(p.take_dirty());
    assert_eq!(p.grid_size(), (20, 4));
}

#[test]
fn resize_signals_session_resize_need() {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    assert!(!p.resize(20, 4));
    assert!(p.resize(30, 8));
    assert_eq!(p.grid_size(), (30, 8));
}

#[test]
fn cells_for_px_clamps_and_divides() {
    assert_eq!(cells_for_px(800.0, 400.0, 8, 16), (100, 25));
    // Collapsed pane never yields a zero grid.
    assert_eq!(cells_for_px(0.0, 0.0, 8, 16), (2, 1));
}

// A pane whose surface is `cols`×`rows` logical px → exactly 1px per cell, so a hover at
// `(col + 0.5, row + 0.5)` lands squarely on cell `(col, row)`.
fn unit_pane(cols: usize, rows: usize) -> TerminalPane {
    TerminalPane::new(cols, rows, Box::new(SoftwareRenderer::new()))
}

#[test]
fn link_at_hits_a_verified_path_and_misses_prose() {
    let dir = std::env::temp_dir().join(format!("hp_pane_link_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("note.txt"), b"hi").unwrap();

    let mut p = unit_pane(40, 3);
    p.set_cwd(Some(dir.to_string_lossy().into_owned()));
    p.feed("see note.txt"); // "see " = cols 0..4, "note.txt" = cols 4..12
    let (w, h) = (40.0, 3.0); // 1px per cell

    // Over the path token → a hit pointing at the absolute file.
    let hit = p
        .link_at(6.5, 0.5, w, h)
        .expect("hover over note.txt should hit");
    assert!(hit.abs_path.replace('\\', "/").ends_with("note.txt"));
    // Underline spans exactly the token's columns (4..12) at 1px/col.
    assert_eq!(hit.x, 4.0);
    assert_eq!(hit.w, 8.0);

    // Over the bare word "see" (not path-shaped) → nothing.
    assert!(p.link_at(1.5, 0.5, w, h).is_none());
    // Over a blank cell past the text → nothing.
    assert!(p.link_at(30.5, 0.5, w, h).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn link_at_ignores_a_nonexistent_path() {
    let dir = std::env::temp_dir().join(format!("hp_pane_nolink_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = unit_pane(40, 2);
    p.set_cwd(Some(dir.to_string_lossy().into_owned()));
    p.feed("ghost.txt is gone"); // shape-passes but doesn't exist → no link
    assert!(p.link_at(2.5, 0.5, 40.0, 2.0).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn link_at_hits_a_url_without_disk_verification() {
    let mut p = unit_pane(40, 3);
    // No cwd set, nothing on disk — a URL must still linkify on shape alone.
    p.feed("go https://a.com/x?q=1 now"); // "go " = cols 0..3, URL = cols 3..22
    let (w, h) = (40.0, 3.0); // 1px per cell

    let hit = p
        .link_at(10.5, 0.5, w, h)
        .expect("hover over the URL should hit");
    assert!(hit.is_url);
    assert_eq!(hit.abs_path, "https://a.com/x?q=1");
    assert_eq!(hit.tip, "https://a.com/x?q=1");
    assert_eq!((hit.line, hit.col), (None, None));
    // Underline spans exactly the URL's columns (3..22) at 1px/col.
    assert_eq!(hit.x, 3.0);
    assert_eq!(hit.w, 19.0);

    // Over the bare word "go" → nothing; past the URL → nothing.
    assert!(p.link_at(1.5, 0.5, w, h).is_none());
    assert!(p.link_at(30.5, 0.5, w, h).is_none());
}

#[test]
fn prepare_paste_normalizes_newlines_to_cr() {
    // LF and CRLF both collapse to CR (the Enter the Windows console expects); a paste with
    // no bracketed-paste mode is sent bare.
    assert_eq!(prepare_paste("a\nb\r\nc", false), "a\rb\rc");
    // Trailing blank lines (e.g. a multi-row selection over empty rows) become trailing CRs.
    assert_eq!(prepare_paste("x\n\n", false), "x\r\r");
    // No newlines → unchanged.
    assert_eq!(prepare_paste("echo hello", false), "echo hello");
}

#[test]
fn prepare_paste_wraps_in_brackets_only_when_mode_on() {
    assert_eq!(prepare_paste("a\nb", true), "\u{1b}[200~a\rb\u{1b}[201~");
    assert!(!prepare_paste("a\nb", false).contains("200~"));
}

#[test]
fn a_click_twitch_within_threshold_is_not_a_drag() {
    // A press with a sub-threshold wobble (even one that crosses a cell boundary) must NOT
    // promote to a drag — otherwise it copies-on-select and clobbers the clipboard before a
    // paste. A 10×10 pane driven by a 100px surface → each cell is 10px wide: a 2px move from
    // x=19 (cell 1) to x=21 (cell 2) straddles a boundary but stays inside the 4px dead zone.
    let mut p = unit_pane(10, 10);
    p.selection_begin(19.0, 5.0, 100.0, 100.0);
    p.selection_update(21.0, 5.0, 100.0, 100.0); // 2px move, crosses cell 1→2
    assert!(
        !p.selection_is_drag(),
        "a sub-threshold twitch must not be a drag"
    );
    assert!(
        p.selection_text().is_none(),
        "a click must not yield copyable text"
    );
    assert!(
        p.selection_rects(100.0, 100.0).is_empty(),
        "a click leaves no highlight"
    );
}

#[test]
fn a_real_drag_past_threshold_selects_and_copies() {
    let mut p = unit_pane(10, 10);
    p.feed("abcdefghij"); // row 0 cells 0..10
    p.selection_begin(5.0, 5.0, 100.0, 100.0); // cell 0
    p.selection_update(55.0, 5.0, 100.0, 100.0); // 50px move → well past threshold, cell 5
    assert!(
        p.selection_is_drag(),
        "a real drag past the threshold selects"
    );
    assert_eq!(p.selection_text().as_deref(), Some("abcdef"));
}

#[test]
fn wheel_scrolls_scrollback_on_the_main_screen() {
    // Grow scrollback past the viewport, then wheel up: the main screen has no app mouse grab,
    // so the wheel moves OUR viewport (no bytes forwarded) and the display offset advances.
    let mut p = unit_pane(20, 3);
    for _ in 0..50 {
        p.feed("line\r\n");
    }
    assert_eq!(p.scroll_offset(), 0, "pinned to the live edge initially");
    assert!(
        p.wheel(3, 5.0, 5.0, 20.0, 3.0).is_none(),
        "no pty forward on the main screen"
    );
    assert!(
        p.scroll_offset() > 0,
        "wheel moved the scrollback viewport up"
    );
}

#[test]
fn wheel_in_alt_screen_without_mouse_forwards_arrow_keys() {
    // Alt screen, no mouse mode → alternate scroll: wheel becomes Up/Down arrows so a pager
    // scrolls. This is the "can't scroll Claude" fix's no-mouse-grab leg.
    let mut p = unit_pane(20, 3);
    p.feed("\x1b[?1049h"); // enter alternate screen
    let up = p
        .wheel(3, 5.0, 5.0, 20.0, 3.0)
        .expect("alt screen forwards to the pty");
    assert_eq!(
        up, b"\x1b[A\x1b[A\x1b[A",
        "3 lines up → three normal Up arrows"
    );
    let down = p.wheel(-3, 5.0, 5.0, 20.0, 3.0).unwrap();
    assert_eq!(down, b"\x1b[B\x1b[B\x1b[B");
    // The viewport never moved (alt screen has no scrollback to scroll).
    assert_eq!(p.scroll_offset(), 0);
}

#[test]
fn wheel_with_app_cursor_keys_uses_ss3_arrows() {
    let mut p = unit_pane(20, 3);
    p.feed("\x1b[?1049h\x1b[?1h"); // alt screen + DECCKM (application cursor keys)
    let up = p.wheel(3, 5.0, 5.0, 20.0, 3.0).unwrap();
    assert_eq!(up, b"\x1bOA\x1bOA\x1bOA", "DECCKM → SS3 Up arrows");
}

#[test]
fn wheel_with_mouse_grab_forwards_sgr_wheel_report() {
    // A mouse-grabbing app (DECSET 1000) with SGR encoding (1006) gets a wheel report at the
    // pointer cell — one per notch — so it (Claude Code / vim / htop) scrolls its own view.
    let mut p = unit_pane(20, 3);
    p.feed("\x1b[?1000h\x1b[?1006h");
    // Pointer over cell (col 4, row 1) → 1-based (5, 2). One notch (3 lines) → one report.
    let up = p.wheel(3, 4.5, 1.5, 20.0, 3.0).unwrap();
    assert_eq!(up, b"\x1b[<64;5;2M", "wheel-up SGR report, button 64");
    let down = p.wheel(-3, 4.5, 1.5, 20.0, 3.0).unwrap();
    assert_eq!(down, b"\x1b[<65;5;2M", "wheel-down SGR report, button 65");
}

#[test]
fn mouse_report_forwards_press_drag_release_to_a_grabbing_app() {
    // DECSET 1002 (button-event/drag tracking) + 1006 (SGR): forward the mouse so the app
    // (Claude) does its own selection. Pointer at cell (4,1) → 1-based (5,2).
    let mut p = unit_pane(20, 3);
    p.feed("\x1b[?1002h\x1b[?1006h");
    assert!(p.app_grabs_mouse());
    // Left press → button 0.
    assert_eq!(
        p.mouse_report(0, 0, 4.5, 1.5, 20.0, 3.0).unwrap(),
        b"\x1b[<0;5;2M"
    );
    // Drag (motion, left held) → +32 motion bit.
    assert_eq!(
        p.mouse_report(1, 0, 4.5, 1.5, 20.0, 3.0).unwrap(),
        b"\x1b[<32;5;2M"
    );
    // Release → SGR final 'm'.
    assert_eq!(
        p.mouse_report(2, 0, 4.5, 1.5, 20.0, 3.0).unwrap(),
        b"\x1b[<0;5;2m"
    );
    // A bare move (no button) under drag-only tracking (1002) is NOT reported.
    assert!(p.mouse_report(1, 3, 4.5, 1.5, 20.0, 3.0).is_none());
}

#[test]
fn mouse_report_is_none_when_app_has_no_mouse_mode() {
    let mut p = unit_pane(20, 3);
    p.feed("hi"); // plain shell, no mouse mode
    assert!(!p.app_grabs_mouse());
    assert!(p.mouse_report(0, 0, 4.5, 1.5, 20.0, 3.0).is_none());
}

#[test]
fn selection_text_follows_content_after_scrolling() {
    // Select a line, then scroll up: the selection is anchored to the absolute line, so its
    // text is unchanged even though the viewport now shows different rows. (Task: keep the
    // selection while scrolling.)
    let mut p = unit_pane(10, 3);
    p.feed("AAAAA\r\n");
    for _ in 0..20 {
        p.feed("xxxxx\r\n");
    }
    // Scroll up so the "AAAAA" line is back on screen, then select its first 5 cells.
    p.scroll_by(100); // clamps to the top of history
                      // Find AAAAA isn't necessary; just select viewport row 0 cols 0..5 and remember its text.
    p.selection_begin(0.5, 0.5, 10.0, 3.0);
    p.selection_update(4.5, 0.5, 10.0, 3.0);
    let before = p.selection_text();
    assert!(before.is_some());
    // Scroll down one line: the same content moves, but selection_text must be identical.
    p.scroll_by(-1);
    assert_eq!(
        p.selection_text(),
        before,
        "selection text is glued to its line"
    );
}

#[test]
fn drag_at_top_edge_autoscrolls_into_history_and_grows_selection() {
    // 60 lines of history in a 20-row pane (so the 8px edge band leaves a middle zone).
    let mut p = unit_pane(10, 20);
    for i in 0..60 {
        p.feed(&format!("L{i}\r\n"));
    }
    // At the live edge; begin a drag at the bottom and pull it to the top row.
    p.selection_begin(0.5, 19.5, 10.0, 20.0);
    p.selection_update(0.5, 0.5, 10.0, 20.0); // y < edge(8) → top band
    assert!(p.selection_is_drag());
    let off0 = p.scroll_offset();
    let grew_before = p.selection_text();
    assert!(p.selection_autoscroll_tick(), "top-edge drag autoscrolls");
    assert!(p.scroll_offset() > off0, "scrolled up into history");
    // The head was re-mapped to the newly revealed top line → the selection text changed.
    assert_ne!(
        p.selection_text(),
        grew_before,
        "selection grew into scrollback"
    );
}

#[test]
fn drag_at_bottom_edge_autoscrolls_toward_live_edge() {
    let mut p = unit_pane(10, 20);
    for i in 0..60 {
        p.feed(&format!("L{i}\r\n"));
    }
    p.scroll_by(30); // scroll up into history first
    let off0 = p.scroll_offset();
    assert!(off0 > 0);
    p.selection_begin(0.5, 0.5, 10.0, 20.0);
    p.selection_update(8.5, 19.5, 10.0, 20.0); // y > sh-edge → bottom band
    assert!(p.selection_is_drag());
    assert!(
        p.selection_autoscroll_tick(),
        "bottom-edge drag autoscrolls"
    );
    assert!(p.scroll_offset() < off0, "scrolled toward the live edge");
    // Releasing the button stops autoscroll even though the selection is kept.
    p.end_selection_drag();
    assert!(
        !p.selection_autoscroll_tick(),
        "no drag in flight → no autoscroll"
    );
}

#[test]
fn no_autoscroll_when_pointer_is_in_the_middle() {
    let mut p = unit_pane(10, 20);
    for i in 0..60 {
        p.feed(&format!("L{i}\r\n"));
    }
    p.scroll_by(15);
    let off0 = p.scroll_offset();
    p.selection_begin(0.5, 9.5, 10.0, 20.0);
    p.selection_update(8.5, 10.5, 10.0, 20.0); // middle band (8..12) — no edge
    assert!(!p.selection_autoscroll_tick(), "middle drag doesn't scroll");
    assert_eq!(p.scroll_offset(), off0);
}

#[test]
fn scrollbar_appears_after_a_scroll_and_is_hidden_at_the_bottom_with_no_history() {
    let mut p = unit_pane(20, 3);
    // No history yet → no scrollbar even right after a (clamped) scroll.
    assert!(p.scrollbar(60.0).is_none(), "no scrollback → no bar");
    for _ in 0..50 {
        p.feed("line\r\n");
    }
    // A fresh scroll shows the bar at full opacity with a thumb shorter than the track.
    p.scroll_by(5);
    let (thumb_y, thumb_h, op) = p.scrollbar(60.0).expect("bar shows right after a scroll");
    assert!(
        (op - 1.0).abs() < 1e-3,
        "fully opaque immediately after scrolling"
    );
    assert!((SCROLLBAR_MIN_THUMB_PX..60.0).contains(&thumb_h));
    assert!(thumb_y >= 0.0);
}

#[test]
fn selection_on_cursor_row_is_prompt_line_only() {
    // 1px/cell on a 20x5 surface. Put the cursor (the "prompt") on row 2.
    let mut p = unit_pane(20, 5);
    p.feed("a\r\nb\r\nprompt"); // rows 0,1,2; cursor ends on row 2
    let (w, h) = (20.0, 5.0);
    assert!(!p.selection_on_cursor_row(), "no selection → false");
    // A single-row drag ON the cursor row is the editable prompt line.
    p.selection_begin(0.5, 2.5, w, h);
    p.selection_update(5.5, 2.5, w, h);
    assert!(p.selection_is_drag());
    assert!(
        p.selection_on_cursor_row(),
        "selection on the cursor row is the prompt line"
    );
    // A drag on a different row is not the prompt line.
    p.selection_begin(0.5, 0.5, w, h);
    p.selection_update(3.5, 0.5, w, h);
    assert!(
        !p.selection_on_cursor_row(),
        "an off-row selection is not the prompt line"
    );
    // A multi-row selection (even one touching the cursor row) is not a single prompt line.
    p.selection_begin(0.5, 0.5, w, h);
    p.selection_update(3.5, 2.5, w, h);
    assert!(
        !p.selection_on_cursor_row(),
        "a multi-row selection is not a prompt line"
    );
    // A stationary click (not dragged) is not a selection.
    p.selection_begin(0.5, 2.5, w, h);
    assert!(
        !p.selection_on_cursor_row(),
        "a click is not a drag-selection"
    );
}

/// 10px/cell on a 20x5 grid: drags between exact cells while clearing the 4px threshold.
fn prompt_pane() -> (TerminalPane, f32, f32) {
    let mut p = unit_pane(20, 5);
    p.feed("a\r\nb\r\nprompt"); // cursor ends at row 2, col 6
    (p, 200.0, 50.0)
}

fn drag(p: &mut TerminalPane, c1: usize, c2: usize, row: usize, w: f32, h: f32) {
    let y = row as f32 * 10.0 + 5.0;
    p.selection_begin(c1 as f32 * 10.0 + 5.0, y, w, h);
    p.selection_update(c2 as f32 * 10.0 + 5.0, y, w, h);
    assert!(p.selection_is_drag());
}

#[test]
fn type_over_backspaces_a_selection_left_of_the_caret() {
    // Select all of "prompt" (cols 0..=5); caret at col 6, right after it → 6 backspaces.
    let (mut p, w, h) = prompt_pane();
    drag(&mut p, 0, 5, 2, w, h);
    assert_eq!(p.type_over_selection(), Some(vec![0x7f; 6]));
    assert!(!p.selection_is_drag(), "type-over consumes the selection");
}

#[test]
fn type_over_steps_left_across_a_gap_before_backspacing() {
    // Select "ro" (cols 1..=2); caret at col 6 → 3 lefts to land after the selection, 2 BS.
    let (mut p, w, h) = prompt_pane();
    drag(&mut p, 1, 2, 2, w, h);
    assert_eq!(
        p.type_over_selection(),
        Some(b"\x1b[D\x1b[D\x1b[D\x7f\x7f".to_vec())
    );
}

#[test]
fn type_over_forward_deletes_a_selection_right_of_the_caret() {
    // Select cols 8..=9 (right of the caret at col 6) → 2 rights + 2 forward-deletes.
    let (mut p, w, h) = prompt_pane();
    drag(&mut p, 8, 9, 2, w, h);
    assert_eq!(
        p.type_over_selection(),
        Some(b"\x1b[C\x1b[C\x1b[3~\x1b[3~".to_vec())
    );
}

#[test]
fn type_over_splits_around_a_caret_inside_the_selection() {
    // Select cols 4..=8 with the caret at col 6 → 2 BS (cols 4-5) + 3 FDEL (cols 6-8).
    let (mut p, w, h) = prompt_pane();
    drag(&mut p, 4, 8, 2, w, h);
    assert_eq!(
        p.type_over_selection(),
        Some(b"\x7f\x7f\x1b[3~\x1b[3~\x1b[3~".to_vec())
    );
}

#[test]
fn type_over_declines_off_row_and_keeps_the_selection() {
    // A selection on a non-cursor row is not editable text — no bytes, selection intact
    // (the caller just clears the highlight).
    let (mut p, w, h) = prompt_pane();
    drag(&mut p, 0, 3, 0, w, h);
    assert_eq!(p.type_over_selection(), None);
    assert!(
        p.selection_is_drag(),
        "declining must not consume the selection"
    );
}

#[test]
fn type_over_spans_a_soft_wrapped_input_line() {
    // 30 chars on a 20-col grid soft-wrap onto row 1 (cursor row 1, col 10). A selection
    // up on row 0 is STILL the same editable line — erase distances go linear through the
    // wrap: caret lin=30, selection cols 5..=8 lin → 21 lefts + 4 backspaces.
    let mut p = unit_pane(20, 5);
    p.feed("abcdefghijklmnopqrstuvwxyz0123");
    let (w, h) = (200.0, 50.0);
    drag(&mut p, 5, 8, 0, w, h);
    let mut expect = b"\x1b[D".repeat(21);
    expect.extend_from_slice(&[0x7f; 4]);
    assert_eq!(p.type_over_selection(), Some(expect));
}

#[test]
fn type_over_declines_on_the_alternate_screen() {
    // In a TUI (vim/htop) the erase bytes would be app commands, not line edits.
    let (mut p, w, h) = prompt_pane();
    p.feed("\x1b[?1049h"); // enter the alternate screen
    drag(&mut p, 0, 5, 2, w, h);
    assert_eq!(p.type_over_selection(), None);
}
