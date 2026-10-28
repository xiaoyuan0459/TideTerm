use tiderm_terminal::{SoftwareRenderer, TerminalPane};

fn pane() -> TerminalPane {
    let mut p = TerminalPane::with_history(40, 8, Box::new(SoftwareRenderer::new()), 2, 4096);
    p.command_startup_begin();
    p
}

#[test]
fn startup_is_source_backed_foldable_and_never_includes_the_first_prompt_or_command() {
    let mut p = pane();
    p.feed("Windows PowerShell\r\nCopyright fixture\r\n\r\n\x1b]133;A\x07PS D:\\fixture> \x1b]133;B\x07");
    let startup = &p.command_blocks()[0];
    let id = startup.id;
    assert!(startup.startup && startup.finished && startup.foldable);
    assert_eq!(
        p.command_output(id, false).unwrap().0,
        "Windows PowerShell\nCopyright fixture\n"
    );
    assert_eq!(p.command_fold_markers(), vec![(id, 0, false)]);
    assert!(p.command_set_folded(id, true));
    assert!(!p.screen_text().contains("Copyright fixture"));
    assert!(p.screen_text().contains("PS D:\\fixture>"));
    assert_eq!(p.command_fold_markers(), vec![(id, 0, true)]);
    assert!(p.command_set_folded(id, false));
    assert!(p.screen_text().contains("Copyright fixture"));
    p.feed("echo FIRST\r\n\x1b]133;C\x07result\r\n\x1b]133;D;0\x07\x1b]133;A\x07PS D:\\fixture> ");
    let blocks = p.command_blocks();
    assert_eq!(blocks.len(), 2);
    assert!(!blocks[0].startup && blocks[0].title == "echo FIRST");
    assert_eq!(
        p.command_output(id, false).unwrap().0,
        "Windows PowerShell\nCopyright fixture\n"
    );
}

#[test]
fn startup_tracks_reflow_and_eviction_and_does_not_rearm_after_clear_or_alternate_screen() {
    let mut p = pane();
    p.feed("\x1b[2J\x1b[Hbanner 中文\r\nsecond\r\nthird\r\nuser@host:~$ ");
    assert!(p.command_startup_pending());
    assert!(p.command_startup_finish());
    let id = p.command_blocks()[0].id;
    let text = p.command_output(id, false).unwrap().0;
    p.resize(20, 8);
    assert_eq!(p.command_output(id, false).unwrap().0, text);
    p.command_set_folded(id, true);
    p.search_set_query("second");
    assert!(p.screen_text().contains("second"));
    for n in 0..1200 {
        p.feed(&format!("ROW_{n}\r\n"));
    }
    assert!(p.command_output(id, false).is_none());
    p.clear();
    p.feed("later\r\noutput\r\n\x1b]133;A\x07$ ");
    assert!(!p.command_blocks().iter().any(|b| b.startup));
    let mut p = pane();
    p.feed("\x1b[?1049hTUI\x1b[?1049l\x1b]133;A\x07$ ");
    assert!(!p.command_startup_pending() && p.command_blocks().is_empty());
    let mut p = pane();
    p.feed("\x1b]133;A\x07$ ");
    assert!(p.command_blocks().is_empty());
}

#[test]
fn startup_disclosure_attaches_to_first_content_after_scrolled_blank_preamble() {
    let mut p = pane();
    p.feed(&"\r\n".repeat(12));
    p.feed("version fixture\r\nedition fixture\r\n\r\n\x1b]133;A\x07$ ");
    let startup = &p.command_blocks()[0];
    assert!(
        p.command_fold_markers().iter().any(|m| m.0 == startup.id),
        "visible startup text needs a visible disclosure"
    );
    assert_eq!(
        p.command_output(startup.id, false).unwrap().0,
        "version fixture\nedition fixture\n"
    );
}
