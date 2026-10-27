//! Behavior fixtures for reading: cell positions, visible text, copy and paste.
use tiderm_terminal::{pane::reading::ReadAction, SoftwareRenderer, TerminalPane};

fn pane() -> TerminalPane {
    let mut pane =
        TerminalPane::with_history(32, 8, Box::new(SoftwareRenderer::new()), 4, 1024 * 1024);
    pane.feed("alpha.beta second\r\n  third sentence. Next!\r\n\r\n中文 🦀 e\u{301} tail\r\nlast");
    pane.set_reading_mode(true);
    keys(&mut pane, "gg");
    pane
}
fn keys(p: &mut TerminalPane, text: &str) -> ReadAction {
    let mut action = ReadAction::None;
    for c in text.chars() {
        action = p.reading_key(0, 0, &c.to_string());
    }
    action
}
fn col(p: &TerminalPane) -> usize {
    p.caret_cell().unwrap().0
}

#[test]
fn counted_words_inline_finders_and_cancel_move_by_terminal_cells() {
    let mut p = pane();
    keys(&mut p, "3l");
    assert_eq!(col(&p), 3);
    keys(&mut p, "w");
    assert_eq!(col(&p), 5);
    keys(&mut p, "w");
    assert_eq!(col(&p), 6);
    keys(&mut p, "0W");
    assert_eq!(col(&p), 11);
    keys(&mut p, "B");
    assert_eq!(col(&p), 0);
    keys(&mut p, "e");
    assert_eq!(col(&p), 4);
    keys(&mut p, "2e");
    assert_eq!(col(&p), 9);
    keys(&mut p, "wge");
    assert_eq!(col(&p), 9);
    keys(&mut p, "0t.");
    assert_eq!(col(&p), 4);
    keys(&mut p, "f.");
    assert_eq!(col(&p), 5);
    keys(&mut p, "fa");
    assert_eq!(col(&p), 9);
    keys(&mut p, ",");
    assert_eq!(col(&p), 4);
    keys(&mut p, "7g");
    p.reading_key(15, 0, "");
    keys(&mut p, "l");
    assert_eq!(col(&p), 5);
    keys(&mut p, "4G0");
    assert_eq!(p.caret_cell(), Some((0, 3)));
    keys(&mut p, "l");
    assert_eq!(col(&p), 2, "wide spacer is not a cursor stop");
    keys(&mut p, "2l");
    assert_eq!(col(&p), 5);
    keys(&mut p, "l");
    assert_eq!(col(&p), 7, "emoji advances two cells");
    keys(&mut p, "gg0yfZw");
    assert_eq!(
        col(&p),
        5,
        "a failed find cancels the pending copy operation"
    );
}

#[test]
fn copy_ranges_registers_and_bracketed_paste_keep_exact_text() {
    let mut p = pane();
    keys(&mut p, "\"ayw");
    let ReadAction::Paste { bytes, stay } = keys(&mut p, "\"aP") else {
        panic!("missing register paste");
    };
    assert_eq!(bytes, b"alpha");
    assert!(stay);
    p.feed_bytes(b"\x1b[?2004h");
    keys(&mut p, "4G0v$");
    assert_eq!(p.selection_text().as_deref(), Some("中文 🦀 e\u{301} tail"));
    keys(&mut p, "y");
    let ReadAction::Paste { bytes, stay } = keys(&mut p, "p") else {
        panic!("missing selection paste");
    };
    assert_eq!(bytes, "\x1b[200~中文 🦀 e\u{301} tail\x1b[201~".as_bytes());
    assert!(!stay);
    keys(&mut p, "ggyy");
    let ReadAction::Paste { bytes, .. } = keys(&mut p, "P") else {
        panic!("missing line paste");
    };
    assert_eq!(bytes, b"\x1b[200~alpha.beta second\r\x1b[201~");
    let mut wrapped =
        TerminalPane::with_history(8, 5, Box::new(SoftwareRenderer::new()), 4, 1024 * 1024);
    wrapped.feed("abcdefghijklmnopq");
    wrapped.set_reading_mode(true);
    keys(&mut wrapped, "gg0vG$");
    assert_eq!(
        wrapped.selection_text().as_deref(),
        Some("abcdefghijklmnopq")
    );
    let mut wide =
        TerminalPane::with_history(8, 5, Box::new(SoftwareRenderer::new()), 4, 1024 * 1024);
    wide.feed("abcde中🦀x");
    wide.set_reading_mode(true);
    keys(&mut wide, "gg0vj$");
    assert_eq!(wide.selection_text().as_deref(), Some("abcde中🦀x"));
    for _ in 0..40 {
        wide.feed("\r\noutput");
    }
    wide.reading_key(15, 0, "");
    keys(&mut wide, "gg0vj$");
    assert_eq!(wide.selection_text().as_deref(), Some("abcde中🦀x"));
}

#[test]
fn scroll_and_output_preserve_reading_selection_and_search_caret() {
    let mut p = pane();
    keys(&mut p, "0v4l");
    let copied = p.selection_text();
    for i in 0..35 {
        p.feed(&format!("\r\noutput-{i:02}"));
    }
    assert_eq!(p.selection_text(), copied);
    assert_eq!(p.caret_cell(), Some((4, 0)));
    p.reading_key(15, 0, "");
    keys(&mut p, "GzzH");
    assert_eq!(p.caret_cell().unwrap().1, 0);
    p.reading_key(0, 4, "b");
    assert!(p.scroll_offset() > 0);
    p.search_open();
    p.search_set_query("output-12");
    assert_eq!(p.caret_cell().unwrap().0, 0);
    assert!(p.screen_text().contains("output-12"));
    p.feed_bytes(b"\x1b[?1049h\x1b[HALT");
    p.reading_scroll(9999);
    assert_eq!(p.scroll_offset(), 0);
    assert!(p.screen_text().contains("ALT"));
    p.feed_bytes(b"\x1b[?1049l");
    assert!(p.caret_cell().is_some());
}

#[test]
fn sentence_and_paragraph_motions_find_content_boundaries() {
    let mut p = pane();
    keys(&mut p, "+");
    assert_eq!(p.caret_cell(), Some((2, 1)));
    keys(&mut p, ")");
    assert_eq!(p.caret_cell(), Some((18, 1)));
    keys(&mut p, "(");
    assert_eq!(p.caret_cell(), Some((2, 1)));
    keys(&mut p, "gg}");
    assert_eq!(p.caret_cell(), Some((0, 3)));
    keys(&mut p, "{");
    assert_eq!(p.caret_cell(), Some((2, 1)));
}

#[test]
fn visual_columns_copy_to_register_without_switching_to_remote_input() {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    p.feed("abcd\r\nABCD");
    p.set_reading_mode(true);
    keys(&mut p, "gg");
    assert!(matches!(p.reading_key(0, 4, "\u{16}"), ReadAction::None));
    keys(&mut p, "llj");
    assert_eq!(p.selection_text().as_deref(), Some("abc\nABC"));
    assert!(matches!(keys(&mut p, "y"), ReadAction::None));
    match keys(&mut p, "P") {
        ReadAction::Paste { bytes, stay } => {
            assert_eq!(bytes, b"abc\rABC");
            assert!(stay);
        }
        _ => panic!("visual column copy must populate the local reading register"),
    }
    keys(&mut p, "gg");
    p.reading_key(0, 4, "v");
    keys(&mut p, "lj");
    p.reading_key(15, 0, "");
    assert_eq!(p.selection_text(), None);
}

#[test]
fn reading_o_swaps_the_visual_ends_without_changing_the_text() {
    for from_menu in [false, true] {
        let swap = |pane: &mut TerminalPane| {
            if from_menu {
                assert!(pane.selection_swap_anchor());
            } else {
                keys(pane, "o");
            }
        };
        let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
        p.feed("abcdef\r\nABCDEF");
        p.set_reading_mode(true);
        keys(&mut p, "gg");
        p.reading_key(0, 0, "v");
        keys(&mut p, "ll");
        assert_eq!(p.selection_text().as_deref(), Some("abc"));
        let before = p.selection_text().unwrap();

        // `o` flips which end is anchored: the copied text is unchanged, but the caret jumps to the
        // other end, so subsequent motions grow the selection from there instead.
        swap(&mut p);
        assert_eq!(p.selection_text().unwrap(), before);
        assert_eq!(
            p.caret_cell(),
            Some((0, 0)),
            "the caret jumped to the other end"
        );
        // The anchor now sits at col 2 and the caret at col 0, so moving right walks the head back
        // across the anchor — the range shrinks, which is what "grow from the other end" means here.
        keys(&mut p, "ll");
        assert_eq!(p.selection_text().as_deref(), Some("c"));
        // Swapping again re-anchors at col 0 and puts the caret back on the far side.
        swap(&mut p);
        assert_eq!(p.selection_text().as_deref(), Some("c"));
        keys(&mut p, "hh");
        assert_eq!(p.selection_text().as_deref(), Some("abc"));
        assert_eq!(p.caret_cell(), Some((0, 0)));

        // Start a fresh visual selection, then test columns independently of the stream.
        p.reading_key(15, 0, "");
        keys(&mut p, "gg");
        assert!(matches!(p.reading_key(0, 4, "\u{16}"), ReadAction::None));
        keys(&mut p, "llj");
        let rect = p.selection_text().unwrap();
        swap(&mut p);
        assert_eq!(
            p.selection_text().unwrap(),
            rect,
            "a rectangle has no direction to flip"
        );
        // The pointer/menu endpoint must become the reading caret: the next motion continues
        // from that endpoint instead of restoring a stale selection snapshot.
        assert!(p.selection_extend_to(45.0, 10.0, 200.0, 80.0, false));
        assert_eq!(p.caret_cell(), Some((4, 0)));
        assert_eq!(p.selection_text().as_deref(), Some("cde\nCDE"));
        keys(&mut p, "h");
        assert_eq!(p.selection_text().as_deref(), Some("cd\nCD"));
        p.reading_key(15, 0, "");
        assert_eq!(p.selection_text(), None);
    }
}

#[test]
fn reading_o_without_a_visual_selection_is_inert() {
    let mut p = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    p.feed("abcdef");
    p.set_reading_mode(true);
    keys(&mut p, "gg");
    let caret = p.caret_cell();
    keys(&mut p, "o");
    assert_eq!(
        p.caret_cell(),
        caret,
        "`o` must not move the caret outside visual mode"
    );
    assert_eq!(p.selection_text(), None);
}
