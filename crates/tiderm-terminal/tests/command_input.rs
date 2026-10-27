//! Editable Shell spans must never include prompt decoration or a running program.
use tiderm_terminal::{SoftwareRenderer, TerminalPane};

fn pane() -> TerminalPane {
    TerminalPane::new(12, 12, Box::new(SoftwareRenderer::new()))
}

#[test]
fn marked_input_preserves_unicode_spaces_reflow_and_retires_on_submission() {
    let mut p = pane();
    p.feed("\x1b]133;A\x07装饰➜ ");
    assert!(
        !p.has_command_input_markers(),
        "A-only hooks cannot provide exact input"
    );
    p.feed("\x1b]133;B\x07echo 中文e\u{301}  ");
    assert!(p.has_command_input_markers());
    assert_eq!(
        p.command_input_prefix().as_deref(),
        Some("echo 中文e\u{301}  ")
    );
    p.resize(9, 12);
    assert_eq!(
        p.command_input_prefix().as_deref(),
        Some("echo 中文e\u{301}  ")
    );
    p.feed("\x1b[2D");
    assert!(p.command_input_prefix().is_none());
    assert_eq!(p.command_input_line().as_deref(), Some("echo 中文e\u{301}"));
    p.command_submitted("echo 中文e\u{301}");
    assert!(p.command_input_line().is_none());
    p.feed("\r\nPassword: ");
    assert!(p.command_input_prefix().is_none());
    p.feed("\x1b]133;D;0\x07\r\n\x1b]133;A\x07% \x1b]133;B\x07");
    assert_eq!(p.command_input_prefix().as_deref(), Some(""));
    p.feed("\x1b[?1049hsecret\x1b[?1049l");
    assert!(p.command_input_prefix().is_none());
    p.clear();
    assert!(
        p.has_command_input_markers(),
        "a clear must not reopen heuristic input capture"
    );
    assert!(p.command_input_prefix().is_none());
}

#[test]
fn prompt_repaint_at_input_falls_back_until_markers_resume() {
    let mut p = pane();
    p.feed("\x1b]133;A\x07PS C:\\dir> \x1b]133;B\x07echo draft");
    assert!(p.command_input_active());
    // ConPTY redraws the viewport after a resize with ED2 + home and does not
    // replay OSC B; the visible conventional prompt must stay usable.
    for _ in 0..2 {
        p.feed("\x1b[2J\x1b[HPS C:\\dir> echo draft");
        assert!(!p.command_input_active());
        assert!(!p.has_command_input_markers());
        assert_eq!(
            p.command_prefix().as_deref(),
            Some("PS C:\\dir> echo draft")
        );
    }
    // A repaint fallback is still input state: every execution/retirement
    // boundary must prevent a later prompt-shaped output from reopening it.
    for boundary in [
        "\x1b]133;C\x07",
        "\x1b]133;D\x07",
        "\x1b[?1049h\x1b[?1049l",
        "\x1bc",
    ] {
        let mut retired = pane();
        retired.feed("\x1b]133;A\x07PS C:\\dir> \x1b]133;B\x07draft");
        retired.feed("\x1b[2J\x1b[HPS C:\\dir> draft");
        retired.feed(boundary);
        retired.feed("\r\nPS C:\\dir> secret");
        assert!(
            retired.has_command_input_markers(),
            "retired repaint must keep marker policy: {boundary:?}"
        );
        assert!(!retired.command_input_active());
    }
    let mut cleared = pane();
    cleared.feed("\x1b]133;A\x07PS C:\\dir> \x1b]133;B\x07draft\x1b[2J");
    cleared.clear();
    assert!(cleared.has_command_input_markers());
    // An A-only replay names the prompt block but still supplies no input
    // anchor, so the conventional prompt must remain the fallback.
    p.feed("\x1b]133;A\x07");
    assert!(!p.has_command_input_markers());
    assert_eq!(
        p.command_prefix().as_deref(),
        Some("PS C:\\dir> echo draft")
    );
    // The next B marker restores the strict anchored policy.
    p.feed("\x1b]133;B\x07ls");
    assert!(p.has_command_input_markers() && p.command_input_active());
    // A clear while a command runs has no input anchor and cannot reopen it.
    p.feed("\x1b]133;C\x07\r\n");
    assert_eq!(p.command_blocks().len(), 1);
    p.feed("\x1b[2J\x1b[Houtput");
    assert!(p.has_command_input_markers());
    assert!(p.command_input_prefix().is_none());
}

#[test]
fn synchronized_clear_and_incomplete_markers_keep_parser_order() {
    let mut p = pane();
    p.feed("\x1b[?2026h\x1b]133;A\x07old> \x1b]133;B\x07old\x1b[2J\x1b[H\x1b]133;A\x07➜ \x1b]133;B\x07new ");
    assert!(!p.has_command_input_markers());
    p.feed("\x1b[?2026l");
    assert_eq!(p.command_input_prefix().as_deref(), Some("new "));
    p.feed("\x1b]133;\x18"); // No boundary kind was supplied.
    assert_eq!(p.command_input_prefix().as_deref(), Some("new "));
    p.feed("\x1b]133;C\x07\r\nuser@host:$ ec");
    assert!(p.command_input_prefix().is_none());
    assert_eq!(p.command_blocks()[0].title, "new");
    p.feed("\x1b]133;D\x07");
    assert!(p.command_blocks()[0].finished);
}

#[test]
fn empty_input_anchor_at_margin_is_editable_after_shrink() {
    let mut p = TerminalPane::new(8, 12, Box::new(SoftwareRenderer::new()));
    p.feed("\x1b]133;A\x07PS /tmp>\x1b]133;B\x07");
    assert_eq!(p.command_input_prefix().as_deref(), Some(""));
    p.resize(4, 12);
    assert_eq!(p.command_input_prefix().as_deref(), Some(""));
    p.command_submitted("pwd");
    assert!(!p.command_input_active());
}
