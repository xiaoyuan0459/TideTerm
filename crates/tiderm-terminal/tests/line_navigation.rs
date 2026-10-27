//! Physical buffer lines share the gutter/selection/fold source coordinates.
use tiderm_terminal::{pane::navigation::LineError, SoftwareRenderer, TerminalPane};
fn pane() -> TerminalPane {
    TerminalPane::with_history(32, 8, Box::new(SoftwareRenderer::new()), 2, 8192)
}
fn keys(p: &mut TerminalPane, value: &str) {
    for c in value.chars() {
        p.reading_key(0, 0, &c.to_string());
    }
}

#[test]
fn preview_and_cancel_are_inert_relative_base_is_fixed_and_tokens_are_single_use() {
    let mut p = pane();
    p.feed("one\r\ntwo\r\nthree\r\nfour\r\n");
    let before = p.history_scroll_state();
    let ctx = p.line_begin().unwrap();
    assert_eq!(ctx.current, 5);
    let a = p.line_preview(ctx.id, "-3").unwrap();
    assert_eq!((a.line, a.text.as_str()), (2, "two"));
    let b = p.line_preview(ctx.id, "+1").unwrap();
    assert_eq!(b.line, 6);
    assert!(!p.line_can_commit(ctx.id, a.token));
    assert_eq!(p.history_scroll_state(), before);
    p.line_end(ctx.id);
    assert!(!p.line_can_commit(ctx.id, b.token));
    let ctx = p.line_begin().unwrap();
    let a = p.line_preview(ctx.id, "2").unwrap();
    p.set_reading_mode(true);
    assert!(p.line_commit(ctx.id, a.token));
    assert!(!p.line_commit(ctx.id, a.token));
    keys(&mut p, "v2l");
    assert_eq!(p.selection_text().as_deref(), Some("two"));
}

#[test]
fn parser_rejects_bad_and_out_of_range_inputs_without_clamping_or_reusing_old_preview() {
    let mut p = pane();
    let ctx = p.line_begin().unwrap();
    for query in ["", "1.5", "--1", "+", ":2", "１２", "1 2"] {
        let valid = p.line_preview(ctx.id, "1").unwrap();
        assert_eq!(p.line_preview(ctx.id, query).err(), Some(LineError::Query));
        assert!(!p.line_can_commit(ctx.id, valid.token));
    }
    for query in [
        "0",
        "9",
        "-1",
        "18446744073709551615",
        "+18446744073709551615",
    ] {
        assert_eq!(p.line_preview(ctx.id, query).err(), Some(LineError::Range));
    }
    let zero = p.line_preview(ctx.id, "+0").unwrap();
    assert_eq!(zero.line, ctx.current);
    let later = p.line_begin().unwrap();
    p.line_end(ctx.id);
    assert!(p.line_preview(later.id, "1").is_ok());
}

#[test]
fn prepared_source_survives_output_compression_and_reflow_but_retires_on_eviction_or_reset() {
    let mut p = pane();
    for n in 0..30 {
        p.feed(&format!("ROW_{n:03} 中e\u{301}\r\n"));
    }
    let ctx = p.line_begin().unwrap();
    let target = p.line_preview(ctx.id, "5").unwrap();
    assert!(target.text.contains("ROW_004 中e\u{301}"));
    p.feed("more\r\nmore\r\n");
    p.resize(24, 10);
    assert!(p.line_can_commit(ctx.id, target.token));
    p.set_reading_mode(true);
    p.line_commit(ctx.id, target.token);
    keys(&mut p, "v6l");
    assert_eq!(p.selection_text().as_deref(), Some("ROW_004"));
    let ctx = p.line_begin().unwrap();
    let target = p.line_preview(ctx.id, "1").unwrap();
    for n in 0..1500 {
        p.feed(&format!("NEW_{n}\r\n"));
    }
    assert!(!p.line_can_commit(ctx.id, target.token));
    for reset in ["\x1b[2J", "\x1b[3J", "\x1bc", "\x1b[?1049h"] {
        let ctx = p.line_begin().unwrap();
        let target = p.line_preview(ctx.id, "1").unwrap();
        p.feed(reset);
        assert!(!p.line_can_commit(ctx.id, target.token));
        if reset.ends_with("1049h") {
            assert!(p.line_begin().is_err());
            p.feed("\x1b[?1049l");
        }
    }
    let ctx = p.line_begin().unwrap();
    let target = p.line_preview(ctx.id, "1").unwrap();
    p.clear();
    assert!(!p.line_can_commit(ctx.id, target.token));
}

#[test]
fn line_navigation_reveals_nested_ancestors_and_preserves_full_output() {
    let mut p = pane();
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo LINES\r\n\x1b]133;C\x07");
    for n in 0..16 {
        p.feed(&format!("LINE_{n:02}\r\n"));
    }
    p.feed("\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let root = p.command_blocks()[0].id;
    let output = p.command_output(root, false).unwrap();
    p.set_reading_mode(true);
    keys(&mut p, "gg2jv6j");
    let nested = p.fold_selection().unwrap();
    p.command_set_folded(root, true);
    let ctx = p.line_begin().unwrap();
    let target = p.line_preview(ctx.id, "5").unwrap();
    assert_eq!(target.text, "LINE_03");
    assert!(p.line_commit(ctx.id, target.token));
    assert!(p
        .fold_regions()
        .iter()
        .filter(|r| r.id == nested || r.id == root)
        .all(|r| !r.folded));
    assert_eq!(p.command_output(root, false).unwrap(), output);
}
