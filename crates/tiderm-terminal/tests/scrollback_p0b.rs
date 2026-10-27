//! P0b integration: the compressed long-history store behind the small live window
//! (WindTerm strategy A). Exercises the public `TermGrid` API end-to-end with a tiny
//! `live_window` so draining happens quickly.

use tiderm_terminal::TermGrid;

fn feed_lines(g: &mut TermGrid, n: usize) {
    let mut s = String::with_capacity(n * 8);
    for i in 0..n {
        s.push_str(&format!("L{i:03}\r\n"));
    }
    g.feed(s.as_bytes());
}

/// A 10-col line "L000" reads back as "L000" + 6 trailing spaces (no right-trim).
fn l(i: usize) -> String {
    format!("L{i:03}      ")
}

#[test]
fn long_history_survives_compression_and_reads_consistently() {
    // Live window of 4 raw lines → draining kicks in after a few scrolled lines.
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 30);

    let (hist, rows, off) = g.scroll_metrics();
    assert_eq!(rows, 3);
    assert_eq!(off, 0, "pinned to the live edge after feed");
    // 30 lines fed, 3 on screen → the other 27 must be retained (store + ≤4-line window).
    // The exact total can be 26 or 27 depending on alacritty's trailing-CRLF scroll
    // semantics — what matters is that NOTHING is lost and the order is unbroken.
    assert!(
        hist >= 26,
        "everything scrolled off is retained: got {hist}"
    );

    // The oldest retained line is L000.
    let top = -(hist as i32);
    assert_eq!(g.line_text(top), Some(l(0)));

    // The retained history + screen form an unbroken stream. A trailing CRLF after the
    // last line may scroll once (leaving a blank bottom row), so the strong property is:
    // every fed line appears exactly once, in order, with nothing lost or duplicated.
    let lines = g.history_lines();
    assert!(lines.len() >= hist + rows - 1);
    for (i, (abs, _)) in lines.iter().enumerate() {
        assert_eq!(*abs, top + i as i32, "absolute lines must be contiguous");
    }
    let non_blank: Vec<String> = lines
        .iter()
        .map(|(_, t)| t.clone())
        .filter(|t| !t.trim().is_empty())
        .collect();
    let expected: Vec<String> = (0..30).map(l).collect();
    assert_eq!(
        non_blank, expected,
        "every fed line present exactly once, in order"
    );
}

#[test]
fn scroll_to_top_shows_oldest_compressed_rows() {
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 20);
    g.scroll_to_top();

    let (hist, rows, off) = g.scroll_metrics();
    assert_eq!(off, hist, "pinned at the top of the total history");
    assert!(hist > rows, "history extends into the compressed store");

    let snap = g.snapshot();
    for r in 0..rows {
        assert_eq!(snap.cell(0, r).ch, 'L', "row {r} comes from the store");
    }
    assert!(
        !snap.cursor_visible,
        "no cursor when scrolled into the store"
    );
}

#[test]
fn scroll_roundtrip_between_store_and_live_edge() {
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 20);
    g.scroll_to_top();
    let top = g.virtual_offset();
    assert!(top > 0, "scrolled into history");

    g.scroll_by(-(top as i32)); // back toward the live edge
    assert_eq!(g.virtual_offset(), 0, "clamped to the live edge");

    let snap = g.snapshot();
    assert!(snap.cursor_visible, "cursor visible again at the live edge");
    // The top two screen rows are filled with the newest lines.
    assert_eq!(snap.cell(0, 0).ch, 'L');
    assert_eq!(snap.cell(0, 1).ch, 'L');
}

#[test]
fn clear_scrollback_empties_store_and_window() {
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 20);
    assert!(g.total_history() > 0);

    // The pane's Clear path: erase display + scrollback, then drop the compressed store.
    g.feed(b"\x1b[H\x1b[2J\x1b[3J");
    g.clear_scrollback();

    assert_eq!(g.total_history(), 0, "raw window and store both empty");
    assert_eq!(g.virtual_offset(), 0);
    let snap = g.snapshot();
    assert!(snap.cursor_visible);
    assert_eq!(snap.cell(0, 0).ch, ' ', "screen cleared");
}

#[test]
fn virtual_offset_clamps_to_retained_history() {
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 8);
    // 8 fed → 3 on screen, ~5 retained. Over-scroll must clamp to the top.
    g.scroll_by(1_000);
    let (hist, _, off) = g.scroll_metrics();
    assert_eq!(off, hist, "over-scroll clamps to the top of history");
    // Over-scroll below the live edge clamps to 0.
    g.scroll_by(-1_000);
    assert_eq!(g.virtual_offset(), 0);
}

/// Alternate-screen commands must never expose the retained primary document.
/// The ordinary alt-screen tests start without compressed history or never scroll.
#[test]
fn alternate_screen_scrolls_stay_inside_the_active_document() {
    let mut g = TermGrid::with_history(10, 3, 4, 1 << 20);
    feed_lines(&mut g, 30);
    let primary_history = g.total_history();
    assert!(primary_history > 4, "exercise compressed primary history");
    g.feed(b"\x1b[?1049h\x1b[HALT");

    let scrolls: [fn(&mut TermGrid); 3] = [
        TermGrid::scroll_to_top,
        |g| g.set_view_offset(usize::MAX),
        |g| g.scroll_by(1_000),
    ];
    for scroll in scrolls {
        scroll(&mut g);
        let snapshot = g.snapshot();
        let visible: String = (0..3).map(|col| snapshot.cell(col, 0).ch).collect();
        assert_eq!(visible, "ALT", "primary history replaced the TUI screen");
        assert_eq!(g.scroll_metrics(), (0, 3, 0));
        assert_eq!(g.total_history(), 0);
        assert_eq!(g.virtual_offset(), 0);
        assert_eq!(snapshot.buffer_lines, 3);
        assert_eq!(snapshot.row_numbers, [1, 2, 3]);
        assert!(snapshot.cursor_visible);
        assert_eq!(g.buffer_lines(), 3);
        assert_eq!(g.output_line_number(), 1);
        assert_eq!(g.history_lines().len(), 3);
        assert_eq!(g.line_text(-1), None);
        assert!(!g.line_wraps(-1));
    }

    g.feed(b"\x1b[?1049l");
    assert_eq!(
        g.total_history(),
        primary_history,
        "primary history was lost"
    );
    g.scroll_to_top();
    assert_eq!(g.line_text(-(primary_history as i32)), Some(l(0)));
    let snapshot = g.snapshot();
    let visible: String = (0..4).map(|col| snapshot.cell(col, 0).ch).collect();
    assert_eq!(visible, "L000");
}

// ---- 存储效率：行尾空白列不入库（2026-10-25 内存专项） ----------------------------

/// A row is stored at the width its content needs, not the grid width. Terminal output is
/// left-aligned, so a 200-column pane spends most of every row on padding; storing it made
/// the byte budget a poor proxy for memory and cut history ~4x shorter than necessary.
#[test]
fn trailing_blank_columns_are_not_stored() {
    let mut g = TermGrid::with_history(200, 3, 4, 1 << 20);
    for i in 0..60 {
        g.feed(format!("L{i:03}\r\n").as_bytes());
    }
    let bytes = g.scrollback_bytes();
    let rows = g.total_history();
    assert!(rows > 30, "expected a long history, got {rows}");
    // 4 painted columns ≈ 8 B of chars + one 20 B run + the 64 B row header. Anything close
    // to 200 columns (400 B of chars) means the trim regressed.
    let per_row = bytes / rows;
    assert!(
        per_row < 160,
        "row costs {per_row} B over {rows} rows — trailing blanks are being stored again"
    );
    // Read paths still see a full-width row: "one char per cell, no right-trim".
    let oldest = -(g.total_history() as i32);
    assert_eq!(g.line_text(oldest).unwrap().len(), 200);
    assert!(g.line_text(oldest).unwrap().starts_with("L000 "));
}

/// Columns that still paint must survive the trim: a coloured background or an underline
/// extends to the last painted column, so the row keeps that width.
#[test]
fn painted_background_and_underline_keep_their_width() {
    let mut g = TermGrid::with_history(20, 3, 4, 1 << 20);
    // Red background across the full 20 columns, then newline.
    g.feed(b"\x1b[41m                    \r\n");
    // Underlined spaces to column 12.
    g.feed(b"\x1b[0m\x1b[4m            \r\n");
    g.feed(b"plain\r\n");
    let oldest = -(g.total_history() as i32);
    assert_eq!(g.line_text(oldest).unwrap(), " ".repeat(20));
    assert_eq!(
        g.line_text(oldest + 1).unwrap(),
        format!("{}{}", " ".repeat(12), " ".repeat(8))
    );
    assert_eq!(g.line_text(oldest + 2).unwrap().trim_end(), "plain");
}

/// A wrapped row made only of padding still carries `WRAPLINE`; the shared blank row does not,
/// so it must be stored on its own (otherwise copy/reflow see a line break that is not there).
#[test]
fn wrapped_blank_rows_keep_their_continuation_flag() {
    let mut g = TermGrid::with_history(8, 3, 4, 1 << 20);
    // 24 spaces on an 8-column grid = three full rows, each wrapped onto the next.
    g.feed(b"                        ");
    // Push them all past the live window into the compressed store.
    for _ in 0..40 {
        g.feed(b"y\r\n");
    }
    let top = -(g.total_history() as i32);
    let wrapped = (top..0).filter(|line| g.line_wraps(*line)).count();
    assert!(
        wrapped >= 2,
        "stored padding rows lost their WRAPLINE flag (wrapped={wrapped})"
    );
}

/// The strongest guard for the trailing-blank trim: a row must render **cell-for-cell** the
/// same after it moves from alacritty's live grid into the compressed store. Compares the
/// whole viewport row (char, fg, bg, all attribute flags) of a styled, wide-glyph line.
#[test]
fn store_rows_render_identically_to_live_rows() {
    let mut g = TermGrid::with_history(20, 4, 2, 1 << 20);
    // Red background + bold, a wide CJK pair, then padding that must not survive the trim.
    g.feed("\x1b[41m\x1b[1mRED\x1b[0m 中文 tail\r\n".as_bytes());
    let live = g.snapshot();
    let live_row: Vec<_> = (0..20)
        .map(|col| {
            let c = *live.cell(col, 0);
            (
                c.ch,
                c.fg,
                c.bg,
                c.bold,
                c.italic,
                c.underline,
                c.wide,
                c.wide_spacer,
            )
        })
        .collect();

    // Push it past the live window (2 lines) into the compressed store.
    for _ in 0..12 {
        g.feed(b"filler\r\n");
    }
    g.set_view_offset(g.total_history());
    let stored = g.snapshot();
    let stored_row: Vec<_> = (0..20)
        .map(|col| {
            let c = *stored.cell(col, 0);
            (
                c.ch,
                c.fg,
                c.bg,
                c.bold,
                c.italic,
                c.underline,
                c.wide,
                c.wide_spacer,
            )
        })
        .collect();
    assert_eq!(
        stored_row, live_row,
        "stored row renders differently from the live row"
    );
}
