use tiderm_terminal::{
    scrollback::{LineData, ScrollbackStore},
    TermGrid,
};

#[test]
fn snapshot_is_complete_immutable_and_does_not_pin_the_live_budget() {
    let mut grid = TermGrid::with_history(140, 3, 4, 64 << 10);
    let prefix = "shared service prefix ".repeat(4);
    let input: String = (0..120)
        .map(|i| format!("{prefix}{i:04} 中😀e\u{301}\r\n"))
        .collect();
    grid.feed(input.as_bytes());
    let snapshot = grid.history_snapshot().expect("primary snapshot");
    let expected = (0..120)
        .map(|i| format!("{prefix}{i:04} 中😀e\u{301}\n"))
        .collect::<String>();
    grid.feed(
        (0..4000)
            .map(|i| format!("replacement {i}\r\n"))
            .collect::<String>()
            .as_bytes(),
    );
    assert!(grid.scrollback_bytes() <= grid.scrollback_budget());
    let mut text = Vec::new();
    snapshot.write_utf8(&mut text, 1 << 20, |_| true).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(
        text.starts_with(&expected),
        "the original source must survive live retirement"
    );
    assert!(!text.contains("replacement"));
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn export_joins_soft_wraps_preserves_unicode_and_honours_failure() {
    let mut grid = TermGrid::with_history(8, 3, 2, 1 << 20);
    grid.feed("\x1b[31m123456中😀e\u{301}尾\x1b[0m\r\nEND".as_bytes());
    let snapshot = grid.history_snapshot().unwrap();
    let mut output = Vec::new();
    snapshot.write_utf8(&mut output, 1024, |_| true).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap().trim_end(),
        "123456中😀e\u{301}尾\nEND"
    );
    assert!(snapshot.write_utf8(&mut Vec::new(), 4, |_| true).is_err());
    assert_eq!(
        snapshot
            .write_utf8(&mut Vec::new(), 1024, |_| false)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    grid.feed(b"\x1b[?1049h");
    assert!(grid.history_snapshot().is_none());
}

fn row(text: &str) -> LineData {
    let mut data = LineData::blank(text.len());
    data.chars = text.encode_utf16().collect::<Vec<_>>().into();
    data
}

#[test]
fn similar_prefixes_keep_more_history_without_changing_rows_or_styles() {
    let mut shared = ScrollbackStore::new(1 << 20);
    let mut varied = ScrollbackStore::new(1 << 20);
    for i in 0..1000 {
        let text = format!("{} {i:04} {}", "p".repeat(64), "z".repeat(60));
        let mut line = row(&text);
        line.runs[0].fg = i;
        shared.push(line);
        let text = format!("{i:04}{} {}{i:04}", "p".repeat(60), "z".repeat(61));
        varied.push(row(&text));
    }
    assert_eq!(shared.rows(), 1000);
    assert!(shared.estimated_bytes() + 100_000 < varied.estimated_bytes());
    for i in [0, 499, 999] {
        let line = shared.row(i).unwrap();
        let text = String::from_utf16(&line.chars.to_arc()).unwrap();
        assert_eq!(
            text,
            format!("{} {i:04} {}", "p".repeat(64), "z".repeat(60))
        );
        assert_eq!(line.runs[0].fg, i as u32);
        let before = shared.estimated_bytes();
        let mut flat = row(&format!("{} {i:04} {}", "p".repeat(64), "z".repeat(60)));
        flat.runs[0].fg = i as u32;
        shared.push(flat);
        assert_eq!(
            shared.estimated_bytes(),
            before,
            "flat and prefix representations must intern as equal text"
        );
    }
    // A reader can retain old rows without keeping them in the store's budget.
    let old = shared.row(0).unwrap();
    for i in 0..20_000 {
        shared.push(row(&format!("replacement {i}")));
    }
    assert!(shared.estimated_bytes() <= shared.budget());
    assert!(String::from_utf16(&old.chars.to_arc())
        .unwrap()
        .starts_with(&"p".repeat(64)));
}

#[test]
fn an_oversized_row_cannot_escape_a_tiny_budget() {
    for budget in [0, 1, 10] {
        let mut store = ScrollbackStore::new(budget);
        store.push(row("oversized"));
        assert_eq!(store.rows(), 0);
        assert_eq!(store.estimated_bytes(), 0);
        store.push(row("again"));
        assert_eq!(store.rows(), 0);
    }
}

#[test]
fn dense_background_batches_preserve_order_and_cannot_republish_a_cleared_generation() {
    let mut input = String::new();
    let mut expected = String::new();
    for row in 0..400 {
        for col in 0..80 {
            let ch = char::from(b'a' + (col % 26) as u8);
            input.push_str(&format!("\x1b[{}m{ch}", 31 + col % 7));
            expected.push(ch);
        }
        input.push_str(&format!("\x1b[0m {row:04}\r\n"));
        expected.push_str(&format!(" {row:04}\n"));
    }
    let mut grid = TermGrid::with_history(96, 3, 4, 8 << 20);
    grid.feed(input.as_bytes());
    let mut text = Vec::new();
    grid.history_snapshot()
        .unwrap()
        .write_utf8(&mut text, 1 << 20, |_| true)
        .unwrap();
    assert!(String::from_utf8(text).unwrap().starts_with(&expected));
    grid.feed(input.as_bytes());
    grid.feed(b"\x1b[2J\x1b[3J\x1b[HNEW_EPOCH");
    // A second grid's read barrier also lets the shared FIFO finish older jobs.
    let mut other = TermGrid::with_history(96, 3, 4, 8 << 20);
    other.feed(input.as_bytes());
    assert!(other.total_history() > 300);
    assert_eq!(grid.total_history(), 0);
    let mut text = Vec::new();
    grid.history_snapshot()
        .unwrap()
        .write_utf8(&mut text, 1024, |_| true)
        .unwrap();
    assert_eq!(String::from_utf8(text).unwrap().trim_end(), "NEW_EPOCH");
}
