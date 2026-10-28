//! Observable visible text is the oracle, across the actual compressed history.
use tiderm_terminal::{SoftwareRenderer, TerminalPane};

fn pane(budget: usize) -> TerminalPane {
    TerminalPane::with_history(18, 5, Box::new(SoftwareRenderer::new()), 4, budget)
}

fn output(start: usize, end: usize) -> Vec<u8> {
    (start..end)
        .map(|i| format!("row-{i:04} 中文\r\n"))
        .collect::<String>()
        .into_bytes()
}

// Existing scroll tests only scroll after output; no test kept reading while
// raw history transferred to the compressed store. Baseline moves these rows.
#[test]
fn scrolled_text_survives_continuous_output_and_history_transfer() {
    let mut p = pane(1024 * 1024);
    p.feed_bytes(&output(0, 25));
    p.scroll_by(12);
    let visible = p.screen_text();
    p.selection_begin(0.0, 0.0, 180.0, 50.0);
    p.selection_update(170.0, 0.0, 180.0, 50.0);
    let selected = p.selection_text();
    assert_eq!(selected.as_deref(), Some("row-0009 中文"));
    for i in 25..85 {
        p.feed_bytes(&output(i, i + 1));
    }
    assert_eq!(
        p.screen_text(),
        visible,
        "incoming output displaced retained reading text"
    );
    assert!(p.total_history() >= 80, "output was stopped or discarded");
    assert_eq!(
        p.selection_text(),
        selected,
        "output displaced selected text"
    );
    p.scroll_to_bottom();
    assert!(p.screen_text().contains("row-0084"));
}

#[test]
fn reading_at_live_edge_retains_unicode_soft_wrapped_text_and_resumes_following() {
    let mut p = pane(1024 * 1024);
    p.feed_bytes(&output(0, 15));
    let top = p.screen_text().lines().next().unwrap().to_owned();
    p.set_follow_output(false);
    p.feed_bytes(
        "长行中文 e\u{301} 🦀 abcdefghijklmnopqrstuvwxyz "
            .repeat(40)
            .as_bytes(),
    );
    assert_eq!(p.screen_text().lines().next(), Some(top.as_str()));
    assert!(p.scroll_offset() > 0);
    p.resize(30, 7);
    assert_eq!(p.screen_text().lines().next(), Some(top.as_str()));
    p.set_follow_output(true);
    p.scroll_to_bottom();
    p.feed_bytes(b"\r\nRESUMED\r\n");
    assert!(p.screen_text().contains("RESUMED"));
    assert_eq!(p.scroll_offset(), 0);
}

#[test]
fn reading_clamps_to_retained_budget_and_alt_screen_continues_updating() {
    let mut p = pane(2048);
    p.feed_bytes(&output(0, 30));
    p.scroll_to_top();
    p.set_follow_output(false);
    let oldest = p.screen_text().lines().next().unwrap().to_owned();
    p.feed_bytes(&output(30, 300));
    let latest = p.screen_text();
    assert_ne!(
        latest.lines().next(),
        Some(oldest.as_str()),
        "evicted rows cannot stay cached"
    );
    assert!(p.scroll_offset() <= p.total_history());
    assert!(!latest.is_empty());
    p.feed_bytes(b"\x1b[?1049h\x1b[HAPP-ONE");
    assert!(p.screen_text().contains("APP-ONE"));
    p.feed_bytes(b"\x1b[HAPP-TWO");
    assert!(p.screen_text().contains("APP-TWO"));
    p.feed_bytes(b"\x1b[?1049l");
    assert!(!p.screen_text().contains("APP-TWO"));
}

// Compression must preserve copyable grapheme payload, rather than just BMP
// screen markers. Older history replaced emoji and dropped combining marks.
#[test]
fn copied_unicode_survives_compression_without_wide_spacers() {
    let mut p = pane(1024 * 1024);
    p.feed_bytes("中文 🦀 e\u{301}\r\n".as_bytes());
    p.feed_bytes(&output(0, 40));
    p.scroll_to_top();
    p.selection_begin(0.0, 0.0, 180.0, 50.0);
    p.selection_update(170.0, 0.0, 180.0, 50.0);
    assert_eq!(p.selection_text().as_deref(), Some("中文 🦀 e\u{301}"));
    p.set_reading_mode(true);
    p.search_open();
    p.search_set_query("e\u{301}");
    assert_eq!(p.search_count(), (1, 1));
    assert_eq!(p.caret_cell().unwrap().0, 8);
    p.search_set_query("🦀");
    assert_eq!(p.caret_cell().unwrap().0, 5);
}
