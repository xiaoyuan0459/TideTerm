//! Peer clear/reset operations retire product history at their parser position.
use tiderm_terminal::{pane::reading::ReadAction, SoftwareRenderer, TermGrid, TerminalPane};

fn grid(window: usize) -> TermGrid {
    TermGrid::with_history(16, 3, window, 1 << 20)
}

fn rows(prefix: &str, count: usize) -> String {
    (0..count).map(|i| format!("{prefix}{i:02}\r\n")).collect()
}

fn row(grid: &TermGrid, index: usize) -> String {
    let snapshot = grid.snapshot();
    snapshot.cells[index * snapshot.cols..(index + 1) * snapshot.cols]
        .iter()
        .map(|cell| cell.ch)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[test]
fn ed3_clears_raw_and_archived_history_without_reusing_old_cached_rows() {
    for window in [1, 8] {
        let mut g = grid(window);
        g.feed(rows("OLD", 8).as_bytes());
        g.scroll_to_top();
        assert_eq!(row(&g, 0), "OLD00");
        // Leading zeros are valid CSI parameters; split exactly at the final byte.
        g.feed(b"\x1b[003");
        g.feed(b"J");
        assert_eq!(g.total_history(), 0, "window={window}");
        assert_eq!(g.virtual_offset(), 0);
        assert_eq!(row(&g, 0), "OLD06");
        assert_eq!(row(&g, 1), "OLD07");
        assert_eq!(g.snapshot().cursor, (0, 2));
        g.feed(rows("NEW", 8).as_bytes());
        g.scroll_to_top();
        assert_eq!(row(&g, 0), "OLD06");
        assert_eq!(row(&g, 1), "OLD07");
        assert_eq!(row(&g, 2), "NEW00");
    }
}

#[test]
fn ris_retires_both_screens_history_and_modes_before_following_output() {
    for alternate in [false, true] {
        let mut g = grid(1);
        g.feed(rows("OLD", 8).as_bytes());
        g.scroll_to_top();
        assert_eq!(row(&g, 0), "OLD00");
        g.feed(b"\x1b[?2004h");
        if alternate {
            g.feed(b"\x1b[?1049hALT");
        }
        g.feed(b"\x1b");
        g.feed(b"cFRESH");
        assert!(!g.alt_screen());
        assert!(!g.bracketed_paste());
        assert_eq!(g.total_history(), 0, "alternate={alternate}");
        assert_eq!(g.virtual_offset(), 0);
        assert_eq!(row(&g, 0), "FRESH");
        g.feed(b"\r\nNEW00\r\nNEW01\r\nNEW02");
        g.scroll_to_top();
        assert_eq!(row(&g, 0), "FRESH");
        assert_eq!(row(&g, 1), "NEW00");
    }
}

#[test]
fn same_feed_retains_only_history_created_after_the_last_reset() {
    // Prefix rows enter deferred compression during this single call. Cleanup
    // must happen at the parsed operation, not after the entire chunk.
    for (reset, synchronized) in [
        ("\x1b[3J", false),
        ("\x1bc", false),
        ("\x1b[3J", true),
        ("\x1bc", true),
    ] {
        let mut g = grid(1);
        let bytes = format!(
            "{}{}\x1b[H{}{}\x1b[H{}",
            rows("OLD", 30),
            reset,
            rows("MID", 30),
            reset,
            rows("NEW", 8)
        );
        if synchronized {
            g.feed(b"\x1b[?2026h");
        }
        g.feed(bytes.as_bytes());
        if synchronized {
            g.feed(b"\x1b[?2026l");
        }
        g.scroll_to_top();
        assert_eq!(row(&g, 0), "NEW00", "reset={reset:?}");
        assert_eq!(row(&g, 1), "NEW01");
    }
}

#[test]
fn peer_ed2_archives_the_whole_screen_with_a_small_raw_window() {
    let mut g = TermGrid::with_history(16, 6, 1, 1 << 20);
    g.feed(b"ROW00\r\nROW01\r\nROW02\r\nROW03\r\nROW04\r\nROW05");
    g.feed(b"\x1b[2J\x1b[H");
    assert_eq!(g.total_history(), 6);
    g.scroll_to_top();
    for i in 0..6 {
        assert_eq!(row(&g, i), format!("ROW{i:02}"));
    }
}

fn pane() -> TerminalPane {
    TerminalPane::with_history(20, 4, Box::new(SoftwareRenderer::new()), 2, 1 << 20)
}

fn command(p: &mut TerminalPane, title: &str, output: &str) {
    p.feed(&format!(
        "\x1b]133;A\x07$ \x1b]133;B\x07{title}\r\n\x1b]133;C\x07{output}\r\n\x1b]133;D;0\x07"
    ));
}

#[test]
fn parsed_ed3_retires_command_selection_search_and_pending_reading_actions() {
    let mut p = pane();
    command(&mut p, "echo OLD", "OLD output");
    let old = p.command_blocks()[0].id;
    p.feed(&rows("fill", 10));
    p.set_reading_mode(true);
    assert!(p.command_jump(old));
    p.search_open();
    p.search_set_query("OLD output");
    assert_eq!(p.search_count().1, 1);
    p.reading_key(0, 0, "v");
    p.reading_key(0, 0, "l");
    let copied = p.selection_text().unwrap();
    p.reading_key(0, 0, "y"); // Copy a known value, then arm a new yank operator.
    p.reading_key(0, 0, "y");
    p.feed("\x1b[03J");
    assert!(p.command_blocks().is_empty());
    assert!(!p.command_jump(old));
    assert_eq!(p.selection_text(), None);
    assert_eq!(p.search_count(), (0, 0));
    assert_eq!(p.history_scroll_state().1, 0);
    p.reading_key(0, 0, "l");
    match p.reading_key(0, 0, "p") {
        ReadAction::Paste { bytes, .. } => assert_eq!(bytes, copied.as_bytes()),
        _ => panic!("the pre-clear yank operator must not overwrite the register"),
    }
    command(&mut p, "echo NEW", "NEW output");
    let new = p.command_blocks()[0].id;
    assert!(new > old);
    assert_eq!(
        p.command_output(new, false),
        Some(("NEW output".into(), false))
    );
}

#[test]
fn synchronized_clear_retires_state_only_when_the_parser_executes_it() {
    let mut p = pane();
    command(&mut p, "echo OLD", "OLD output");
    let old = p.command_blocks()[0].id;
    let history = p.history_scroll_state().1;
    let output = p.command_output(old, false);
    p.feed("\x1b[?2026h\x1b[3J");
    assert_eq!(p.command_output(old, false), output);
    assert_eq!(p.history_scroll_state().1, history);
    p.feed("\x1b[?2026l");
    assert!(p.command_blocks().is_empty());
    assert_eq!(p.history_scroll_state().1, 0);
}
