//! Public history navigation and read-only preview must share the visible projection.
use tiderm_terminal::{SoftwareRenderer, TerminalPane};

#[test]
fn archived_unicode_history_preview_is_bounded_and_does_not_scroll() {
    let mut pane =
        TerminalPane::with_history(16, 4, Box::new(SoftwareRenderer::new()), 2, 1024 * 1024);
    for row in 0..100 {
        pane.feed(&format!("{row:03} 中e\u{301}\r\n"));
    }
    let before = pane.history_scroll_state();
    let (first, text, truncated) = pane.history_preview(0).unwrap();
    assert_eq!(first, 1);
    assert_eq!(
        text,
        (0..32)
            .map(|row| format!("{row:03} 中e\u{301}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(truncated);
    assert_eq!(
        pane.history_scroll_state(),
        before,
        "hover moved the viewport"
    );
    pane.history_scroll_to(0);
    assert!(pane.screen_text().starts_with("000 "));
    pane.history_scroll_to(usize::MAX);
    assert_eq!(pane.history_scroll_state().0, before.1);
    assert!(pane.history_preview(before.1 + 1).is_none());

    pane.feed("\x1b[?1049hALT");
    assert_eq!(pane.history_scroll_state().1, 0);
    assert!(pane.history_preview(0).is_none());
    pane.feed("\x1b[?1049l");
    pane.clear();
    assert_eq!(pane.history_scroll_state().1, 0);
}

#[test]
fn scrollbar_and_preview_skip_folded_output_then_follow_reflow_and_eviction() {
    let mut pane = TerminalPane::with_history(32, 4, Box::new(SoftwareRenderer::new()), 2, 4096);
    pane.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo FIRST\r\n\x1b]133;C\x07OUT_A\r\nHIDDEN_ONE\r\nHIDDEN_TWO\r\nOUT_LAST\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let live_block = pane.command_blocks()[0].id;
    assert!(pane.command_set_folded(live_block, true));
    pane.history_scroll_to(0);
    pane.history_scroll_to(usize::MAX);
    assert_eq!(
        pane.scroll_offset(),
        0,
        "End must restore the live edge after folding current output"
    );
    assert!(pane.command_set_folded(live_block, false));
    for row in 0..12 {
        pane.feed(&format!("tail{row:02}\r\n"));
    }
    let id = pane
        .command_blocks()
        .into_iter()
        .find(|b| b.title == "echo FIRST")
        .unwrap()
        .id;
    let expanded = pane.history_scroll_state().1;
    assert!(pane.command_set_folded(id, true));
    assert!(pane.history_scroll_state().1 < expanded);
    let text = pane.history_preview(0).unwrap().1;
    assert!(text.contains("OUT_A …") && text.contains("tail00"));
    assert!(!text.contains("HIDDEN_ONE") && !text.contains("OUT_LAST"));
    pane.history_scroll_to(0);
    assert!(!pane.screen_text().contains("HIDDEN_ONE"));
    assert!(pane.command_set_folded(id, false));
    assert!(pane.history_preview(0).unwrap().1.contains("HIDDEN_TWO"));
    pane.resize(16, 6);
    assert!(pane.history_preview(0).unwrap().1.contains("echo FIRST"));
    for row in 0..600 {
        pane.feed(&format!("new{row:03}\r\n"));
    }
    assert!(!pane.history_preview(0).unwrap().1.contains("echo FIRST"));
    let (_, maximum, _) = pane.history_scroll_state();
    pane.history_scroll_to(maximum);
    assert_eq!(pane.scroll_offset(), 0);
}

#[test]
fn history_preview_rejects_partial_utf8_at_its_byte_limit() {
    let mut pane =
        TerminalPane::with_history(12000, 2, Box::new(SoftwareRenderer::new()), 2, 1024 * 1024);
    pane.feed(&("a".repeat(8191) + "中tail"));
    let (_, text, truncated) = pane.history_preview(0).unwrap();
    assert!(truncated);
    assert_eq!(text, "a".repeat(8191));
}
