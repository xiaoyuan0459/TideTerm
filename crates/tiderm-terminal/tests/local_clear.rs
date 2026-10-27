//! Local display actions must not become bytes in the peer's unfinished stream.
use tiderm_terminal::{SoftwareRenderer, TermGrid, TerminalPane};

fn pane() -> TerminalPane {
    TerminalPane::with_history(12, 3, Box::new(SoftwareRenderer::new()), 2, 1 << 20)
}

fn grid(window: usize) -> TermGrid {
    TermGrid::with_history(12, 2, window, 1 << 20)
}

fn seed(grid: &mut TermGrid, prefix: &str, rows: usize) {
    for row in 0..rows {
        grid.feed(format!("{prefix}{row:02}\r\n").as_bytes());
    }
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
fn local_clears_preserve_unfinished_output_sequences() {
    struct Action {
        name: &'static str,
        apply: fn(&mut TerminalPane),
    }
    struct Fragment {
        name: &'static str,
        prefix: &'static [u8],
        suffix: &'static [u8],
        text: &'static str,
        title: Option<&'static str>,
    }
    let actions = [
        Action {
            name: "screen",
            apply: TerminalPane::clear_screen,
        },
        Action {
            name: "history",
            apply: TerminalPane::clear_scrollback,
        },
        Action {
            name: "all",
            apply: TerminalPane::clear,
        },
    ];
    let fragments = [
        Fragment {
            name: "UTF-8",
            prefix: b"\xe4\xb8",
            suffix: b"\xad",
            text: "中",
            title: None,
        },
        Fragment {
            name: "CSI",
            prefix: b"\x1b[31",
            suffix: b"mX",
            text: "X",
            title: None,
        },
        Fragment {
            name: "OSC",
            prefix: b"\x1b]2;TITLE",
            suffix: b"-END\x07Q",
            text: "Q",
            title: Some("TITLE-END"),
        },
    ];
    let mut failures = Vec::new();
    for action in actions {
        for fragment in &fragments {
            let mut pane = pane();
            pane.feed_bytes(fragment.prefix);
            (action.apply)(&mut pane);
            pane.feed_bytes(fragment.suffix);
            let actual = pane.screen_text();
            let actual_title = pane.take_title();
            if actual.trim() != fragment.text || actual_title.as_deref() != fragment.title {
                failures.push(format!(
                    "{}/{}: text={:?}, title={actual_title:?}",
                    action.name,
                    fragment.name,
                    actual.trim()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn history_clear_retires_cached_rows_before_archive_indices_are_reused() {
    let mut grid = grid(1);
    seed(&mut grid, "OLD", 8);
    grid.scroll_to_top();
    assert_eq!(row(&grid, 0), "OLD00"); // Populate the archived rendering cache.
    grid.clear_scrollback();
    assert_eq!(grid.total_history(), 0);

    seed(&mut grid, "NEW", 8);
    grid.scroll_to_top();
    // Clearing history preserved the old visible row; new output now archives it.
    assert_eq!(row(&grid, 0), "OLD07");
    assert_eq!(row(&grid, 1), "NEW00");
}

#[test]
fn history_clear_includes_raw_rows_and_preserves_the_visible_screen() {
    let mut grid = grid(8);
    seed(&mut grid, "RAW", 5);
    assert_eq!(grid.total_history(), 4);
    assert_eq!(row(&grid, 0), "RAW04");
    grid.scroll_to_top();
    grid.clear_scrollback();
    assert_eq!(grid.total_history(), 0);
    assert_eq!(grid.virtual_offset(), 0);
    assert_eq!(row(&grid, 0), "RAW04");
    assert_eq!(row(&grid, 1), "");
    assert_eq!(grid.snapshot().cursor, (0, 1));
}

#[test]
fn alternate_history_clear_preserves_the_primary_document() {
    let mut grid = grid(2);
    seed(&mut grid, "MAIN", 8);
    assert_eq!(grid.total_history(), 7);
    grid.feed(b"\x1b[?1049h\x1b[HALT");
    assert_eq!(row(&grid, 0), "ALT");
    grid.clear_scrollback();
    assert_eq!(grid.total_history(), 0);
    assert_eq!(row(&grid, 0), "ALT");
    grid.feed(b"\x1b[?1049l");
    assert_eq!(grid.total_history(), 7);
    grid.scroll_to_top();
    assert_eq!(row(&grid, 0), "MAIN00");
}

#[test]
fn screen_clear_archives_the_whole_visible_screen_with_a_small_raw_window() {
    let mut pane = TerminalPane::with_history(12, 6, Box::new(SoftwareRenderer::new()), 1, 1 << 20);
    pane.feed("ROW00\r\nROW01\r\nROW02\r\nROW03\r\nROW04\r\nROW05");
    pane.clear_screen();
    assert!(pane.screen_text().trim().is_empty());
    assert_eq!(pane.history_scroll_state().1, 6);
    assert_eq!(
        pane.history_preview(0).unwrap().1.trim_end(),
        "ROW00\nROW01\nROW02\nROW03\nROW04\nROW05"
    );
}
