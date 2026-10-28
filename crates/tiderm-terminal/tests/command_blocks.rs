//! Public pane behavior: positions are independent of title-only history and
//! remain attached to retained text, including parser and storage boundaries.
use tiderm_terminal::{SoftwareRenderer, TerminalPane};

fn pane(cols: usize, cap: usize) -> TerminalPane {
    TerminalPane::with_history(cols, 4, Box::new(SoftwareRenderer::new()), 2, cap)
}

fn command(p: &mut TerminalPane, title: &str, output: &str) {
    p.feed(&format!(
        "\x1b]133;A\x07$ \x1b]133;B\x07{title}\r\n\x1b]133;C\x07{output}\x1b]133;D;0\x07"
    ));
}

#[test]
fn split_markers_unicode_softwrap_and_output_ranges() {
    let fixture = "\x1b]133;A\x1b\\$ \x1b]133;B\x07echo 中文e\u{301}abcdef\r\n\x1b]133;C\x1b\\中文e\u{301}abcde0123456789\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ";
    for synchronized in [false, true] {
        let mut p = pane(12, 100_000);
        if synchronized {
            p.feed("\x1b[?2026h");
        }
        for byte in fixture.as_bytes() {
            p.feed_bytes(&[*byte]);
        }
        if synchronized {
            assert!(
                p.command_blocks().is_empty(),
                "buffered markers must wait for parser execution"
            );
            p.feed("\x1b[?2026l");
        }
        let rows = p.command_blocks();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "echo 中文e\u{301}abcdef");
        assert!(rows[0].finished);
        assert_eq!(
            p.command_output(rows[0].id, false),
            Some(("中文e\u{301}abcde0123456789".into(), false))
        );
        p.set_reading_mode(true);
        assert!(p.command_jump(rows[0].id));
        assert!(!p.command_step(true));
    }
    // OSC completion at the right margin is a boundary after the final cell.
    for output in ["0123456789", "中文abcdef"] {
        let mut margin = TerminalPane::new(10, 8, Box::new(SoftwareRenderer::new()));
        command(&mut margin, "e", output);
        let id = margin.command_blocks()[0].id;
        assert_eq!(
            margin.command_output(id, false),
            Some((output.into(), false))
        );
    }
}

#[test]
fn context_command_selection_excludes_next_prompt_and_rejects_retired_identity() {
    let mut p = TerminalPane::new(30, 8, Box::new(SoftwareRenderer::new()));
    command(&mut p, "echo 中文", "first output\r\n");
    p.feed("\x1b]133;A\x07$ ");
    let id = p.command_blocks()[0].id;
    assert_eq!(p.command_at_point(20.0, 25.0, 300.0, 160.0), Some(id));
    assert_eq!(p.command_at_point(20.0, 65.0, 300.0, 160.0), None);
    p.set_reading_mode(true);
    assert!(p.command_select_block(id));
    assert_eq!(
        p.selection_text().as_deref(),
        Some("$ echo 中文\nfirst output")
    );
    p.feed("\x1b[?1049h");
    assert!(!p.command_select_block(id));
    p.feed("\x1b[?1049l");
    p.clear();
    assert!(!p.command_select_block(id));
}

#[test]
fn positions_survive_continuous_output_and_compressed_history() {
    let mut p = pane(30, 1_000_000);
    command(&mut p, "echo FIRST", "FIRST output\r\n");
    let first = p.command_blocks()[0].id;
    p.set_reading_mode(true);
    assert!(p.command_jump(first));
    for i in 0..80 {
        command(&mut p, &format!("echo {i}"), &format!("output {i}\r\n"));
    }
    assert!(p.total_history() > 100);
    assert_eq!(
        p.command_output(first, false),
        Some(("FIRST output".into(), false))
    );
    assert!(p.command_jump(first));
    p.resize(30, 8);
    assert_eq!(
        p.command_output(first, false),
        Some(("FIRST output".into(), false))
    );
    p.resize(30, 4);
    assert_eq!(
        p.command_output(first, false),
        Some(("FIRST output".into(), false))
    );
    assert!(p.command_step(true));
    assert!(p.command_step(false));
    for ch in "zjzk".chars() {
        p.reading_key(0, 0, &ch.to_string());
    }
    assert!(p.command_step(true));
    // Other panes must never expose this pane's positions or output.
    assert!(pane(30, 100_000).command_blocks().is_empty());
}

#[test]
fn eviction_live_reflow_clear_and_alternate_screen_retire_or_hide_positions() {
    let mut p = pane(20, 512);
    command(&mut p, "echo OLD", "old output\r\n");
    let old = p.command_blocks()[0].id;
    for i in 0..80 {
        command(
            &mut p,
            &format!("echo {i}"),
            &format!("unique output {i}\r\n"),
        );
    }
    assert!(!p.command_jump(old));
    assert_eq!(p.command_output(old, false), None);
    let recent = p.command_blocks()[0].id;
    p.feed("\x1b[?1049hpassword\x1b]133;A\x07\x1b]133;B\x07secret\x1b]133;C\x07");
    assert!(p.command_blocks().is_empty());
    assert!(!p.command_jump(recent));
    p.clear_scrollback();
    p.clear_screen();
    p.clear();
    p.feed("\x1b[2J\x1b[H\x1b[3J");
    p.feed("\x1b[?1049l");
    assert!(p.command_blocks().iter().any(|b| b.id == recent));
    p.resize(10, 4);
    assert!(p.command_blocks().iter().any(|b| b.id == recent));
    assert!(p.command_jump(recent));
    assert_eq!(
        p.command_output(recent, false),
        Some(("unique output 79".into(), false))
    );
    command(&mut p, "echo NEW", "new output\r\n");
    let new = p.command_blocks()[0].id;
    assert!(new > recent);
    p.clear_scrollback();
    assert!(!p.command_jump(new));
    assert!(p.command_blocks().is_empty());
    command(&mut p, "echo CLEAR", "clear output\r\n");
    let clear = p.command_blocks()[0].id;
    p.feed("\x1b[2J\x1b[Hnew screen");
    assert!(!p.command_jump(clear));
    assert_eq!(p.command_output(clear, false), None);
}

#[test]
fn command_and_copy_budgets_are_explicit_and_utf8_safe() {
    let mut p = pane(80, 4_000_000);
    command(
        &mut p,
        "echo LARGE",
        &format!("{}\r\n", "中".repeat(30_000)),
    );
    let id = p.command_blocks()[0].id;
    let (preview, truncated) = p.command_output(id, true).unwrap();
    assert!(truncated && preview.len() <= 8192);
    let (copy, truncated) = p.command_output(id, false).unwrap();
    assert!(truncated && copy.len() <= 65536);
    for i in 0..510 {
        command(&mut p, &format!("echo {i}"), "ok\r\n");
    }
    assert_eq!(p.command_blocks().len(), 500);
}

#[test]
fn archived_command_positions_and_partial_row_output_survive_column_resize() {
    let mut p = pane(40, 1_000_000);
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo tail \x1b]133;C\x07RESULT\r\n\x1b]133;D;0\x07");
    let id = p.command_blocks()[0].id;
    assert_eq!(p.command_output(id, false), Some(("RESULT".into(), false)));
    for i in 0..60 {
        p.feed(&format!("line {i}\r\n"));
    }
    p.set_reading_mode(true);
    assert!(p.command_jump(id));
    for cols in [4, 80, 12, 40] {
        assert!(p.resize(cols, 4));
        assert!(p.command_blocks().iter().any(|b| b.id == id));
        assert!(p.command_jump(id));
        assert_eq!(p.command_output(id, false), Some(("RESULT".into(), false)));
    }
    p.clear_scrollback();
    assert!(!p.command_jump(id));
}

#[test]
fn resizing_during_split_output_keeps_the_output_parser_intact() {
    let mut p = pane(30, 100_000);
    for _ in 0..9 {
        p.feed("history\r\n");
    }
    p.feed_bytes(b"\x1b]133;A\x07$ \x1b]133;B\x07echo \xe4");
    p.resize(20, 4);
    p.feed_bytes(b"\xb8\xad\r\n\x1b]133;C\x07OUT\r\n\x1b]133;D\x07");
    assert!(p.screen_text().contains("echo 中"));
    assert!(p.screen_text().contains("OUT"));
}

#[test]
fn live_command_identity_and_output_survive_unicode_reflow() {
    let mut p =
        TerminalPane::with_history(36, 12, Box::new(SoftwareRenderer::new()), 200, 1_000_000);
    let expected = "0123456789中文e\u{301}abcdefghijklmnop\nNEXT";
    command(
        &mut p,
        "printf 中文",
        "0123456789中文e\u{301}abcdefghijklmnop\r\nNEXT\r\n",
    );
    let id = p.command_blocks()[0].id;
    command(&mut p, "echo SECOND", "SECOND\r\n");
    let second = p.command_blocks()[0].id;
    p.set_reading_mode(true);
    for cols in [8, 60, 17, 36] {
        assert!(p.resize(cols, 12));
        assert_eq!(p.command_blocks().len(), 2);
        assert!(p.command_jump(id));
        assert_eq!(p.command_output(id, false), Some((expected.into(), false)));
        assert!(p.command_jump(second));
        assert_eq!(
            p.command_output(second, false),
            Some(("SECOND".into(), false))
        );
    }
}

#[test]
fn folding_projects_output_without_changing_copy_and_search_reveals_hidden_hits() {
    let mut p =
        TerminalPane::with_history(30, 12, Box::new(SoftwareRenderer::new()), 200, 1_000_000);
    command(&mut p, "echo ONE", "HIDDEN-A\r\nHIDDEN-B\r\nHIDDEN-C\r\n");
    let id = p.command_blocks()[0].id;
    command(&mut p, "echo TWO", "VISIBLE\r\nSECOND visible\r\n");
    let two = p.command_blocks()[0].id;
    command(&mut p, "echo THREE", "THIRD\r\n");
    let three = p.command_blocks()[0].id;
    p.set_reading_mode(true);
    assert!(p.command_jump(id));
    assert!(p.command_set_folded(id, true));
    assert!(!p.screen_text().contains("HIDDEN-A"));
    assert!(p.screen_text().contains("echo TWO"));
    assert_eq!(
        p.command_output(id, false),
        Some(("HIDDEN-A\nHIDDEN-B\nHIDDEN-C".into(), false))
    );
    p.search_open();
    p.search_set_query("HIDDEN-B");
    assert_eq!(p.search_count(), (1, 1));
    assert!(
        !p.command_blocks()
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .folded
    );
    assert!(p.screen_text().contains("HIDDEN-B"));
    assert!(p.command_set_folded(id, true));
    for cols in [10, 30] {
        p.resize(cols, 12);
        assert!(
            p.command_blocks()
                .iter()
                .find(|b| b.id == id)
                .unwrap()
                .folded
        );
        assert_eq!(
            p.command_output(id, false),
            Some(("HIDDEN-A\nHIDDEN-B\nHIDDEN-C".into(), false))
        );
    }
    p.feed("\x1b[?1049hTUI");
    assert!(p.screen_text().contains("TUI"));
    assert!(!p.command_set_folded(id, false));
    p.feed("\x1b[?1049l");
    assert!(
        p.command_blocks()
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .folded
    );
    p.command_jump(id);
    for keys in ["zo", "zc", "za", "zO", "zC", "zm", "zR", "zM"] {
        p.reading_key(0, 0, &keys[0..1]);
        p.reading_key(0, 0, &keys[1..2]);
        let closed = matches!(keys, "zc" | "zC" | "zm" | "zM");
        assert_eq!(
            p.command_blocks()
                .iter()
                .find(|b| b.id == id)
                .unwrap()
                .folded,
            closed
        );
    }
    assert!(
        p.command_blocks()
            .iter()
            .find(|b| b.id == two)
            .unwrap()
            .folded
    );
    p.reading_key(0, 0, "j");
    p.reading_key(0, 0, "j");
    assert_eq!(p.caret_cell().unwrap().1, 2);
    for _ in 0..80 {
        p.feed("continuing output\r\n");
        assert_eq!(p.caret_cell().unwrap().1, 2);
    }
    assert!(p.command_jump(id));
    assert!(!p.screen_text().contains("HIDDEN-B"));
    assert_eq!(
        p.command_output(two, false),
        Some(("VISIBLE\nSECOND visible".into(), false))
    );
    assert!(p.command_jump(three));
    assert!(!p.command_fold_current(None));
    p.clear_scrollback();
    assert!(!p.command_set_folded(id, false));
}

#[test]
fn folded_rows_share_pointer_selection_and_reading_navigation_mapping() {
    let mut p =
        TerminalPane::with_history(30, 12, Box::new(SoftwareRenderer::new()), 200, 1_000_000);
    command(&mut p, "echo ONE", "A\r\nB\r\nC\r\nD\r\nE\r\n");
    let id = p.command_blocks()[0].id;
    command(&mut p, "echo TWO", "VISIBLE\r\n");
    p.set_reading_mode(true);
    p.command_jump(id);
    p.command_set_folded(id, true);
    p.reading_key(0, 0, "j");
    p.reading_key(0, 0, "j");
    assert_eq!(p.caret_cell().unwrap().1, 2);
    p.selection_begin(0.0, 30.0, 300.0, 120.0);
    p.selection_update(70.0, 30.0, 300.0, 120.0);
    assert_eq!(p.selection_text().as_deref(), Some("VISIBLE"));
    assert_eq!(p.selection_rects(300.0, 120.0)[0].1, 30.0);
    p.set_reading_mode(false);
    p.feed("$ abcdef\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        p.mouse_report(0, 0, 20.0, 40.0, 300.0, 120.0),
        Some(b"\x1b[<0;3;9M".to_vec())
    );
    assert_eq!(
        p.wheel(-3, 20.0, 40.0, 300.0, 120.0),
        Some(b"\x1b[<65;3;9M".to_vec())
    );
    // A synthetic fold marker must never be sent as an application cell.
    assert!(p.mouse_report(0, 0, 0.0, 10.0, 300.0, 120.0).is_none());
    p.selection_begin(20.0, 40.0, 300.0, 120.0);
    p.selection_update(40.0, 40.0, 300.0, 120.0);
    assert_eq!(p.selection_text().as_deref(), Some("abc"));
    assert!(p.selection_on_cursor_row());
    assert_eq!(
        p.type_over_selection(),
        Some(b"\x1b[D\x1b[D\x1b[D\x7f\x7f\x7f".to_vec())
    );
    assert!(TerminalPane::new(30, 12, Box::new(SoftwareRenderer::new()))
        .command_blocks()
        .is_empty());
}
