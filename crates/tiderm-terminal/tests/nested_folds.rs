//! Completed-command ownership, nested states and source lifecycle are public contracts.
use tiderm_terminal::{pane::command_blocks::FoldError, SoftwareRenderer, TerminalPane};

fn pane(lines: usize, budget: usize) -> TerminalPane {
    let mut p = TerminalPane::with_history(32, 8, Box::new(SoftwareRenderer::new()), 2, budget);
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo TREE\r\n\x1b]133;C\x07");
    for row in 0..lines {
        p.feed(&format!("ROW_{row:03} 中e\u{301}\r\n"));
    }
    p.feed("\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    p
}
fn keys(p: &mut TerminalPane, text: &str) {
    for c in text.chars() {
        p.reading_key(0, 0, &c.to_string());
    }
}
fn select(p: &mut TerminalPane, first: usize, last: usize) {
    p.command_fold_all(false);
    p.set_reading_mode(true);
    keys(p, "gg");
    if first != 0 {
        keys(p, &format!("{first}j"));
    }
    keys(p, "v");
    if last > first {
        keys(p, &format!("{}j", last - first));
    }
}
fn add(p: &mut TerminalPane, first: usize, last: usize) -> u64 {
    select(p, first, last);
    p.fold_selection().unwrap()
}
fn folded(p: &TerminalPane, id: u64) -> bool {
    p.fold_regions().iter().find(|r| r.id == id).unwrap().folded
}

#[test]
fn nested_projection_preserves_child_state_copy_and_explicit_navigation() {
    let mut p = pane(20, 1024 * 1024);
    let root = p.command_blocks()[0].id;
    let original = p.command_output(root, false).unwrap();
    let inner = add(&mut p, 4, 6);
    let outer = add(&mut p, 2, 10);
    assert_eq!(
        p.fold_regions()
            .iter()
            .find(|r| r.id == inner)
            .unwrap()
            .parent,
        outer
    );
    assert_eq!(
        p.fold_regions()
            .iter()
            .find(|r| r.id == inner)
            .unwrap()
            .depth,
        2
    );
    p.fold_region_set(inner, true, false);
    p.history_scroll_to(0);
    let preview = p.history_preview(0).unwrap().1;
    assert!(preview.contains("ROW_001") && !preview.contains("ROW_004"));
    assert_eq!(p.command_output(root, false).unwrap(), original);
    p.fold_region_set(outer, false, false);
    assert!(folded(&p, inner));
    assert!(p.history_preview(0).unwrap().1.contains("ROW_002"));
    assert!(!p.history_preview(0).unwrap().1.contains("ROW_004"));
    p.fold_region_set(outer, true, true);
    assert!(folded(&p, inner));
    p.fold_region_set(outer, false, true);
    assert!(!folded(&p, inner));
    p.fold_region_set(outer, true, false);
    p.fold_region_set(inner, true, false);
    assert!(p.fold_region_jump(inner));
    assert!(!folded(&p, outer) && folded(&p, inner));
    assert!(p.fold_region_remove(outer));
    assert_eq!(
        p.fold_regions()
            .iter()
            .find(|r| r.id == inner)
            .unwrap()
            .parent,
        root
    );
    assert!(!p.fold_region_remove(root));
}

#[test]
fn creation_rejects_crossing_duplicate_rectangle_live_and_oversized_trees() {
    let mut p = pane(420, 4 * 1024 * 1024);
    assert_eq!(p.fold_selection(), Err(FoldError::NoSelection));
    let _one = add(&mut p, 2, 8);
    select(&mut p, 5, 10);
    assert_eq!(p.fold_selection(), Err(FoldError::Crossing));
    select(&mut p, 2, 8);
    assert_eq!(p.fold_selection(), Err(FoldError::Crossing));
    select(&mut p, 0, 3);
    assert_eq!(p.fold_selection(), Err(FoldError::Output));
    p.selection_clear();
    keys(&mut p, "gg");
    p.reading_key(0, 4, "\u{16}");
    keys(&mut p, "2j");
    assert_eq!(p.fold_selection(), Err(FoldError::Shape));
    p.clear();
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07run\r\n\x1b]133;C\x07one\r\ntwo\r\nthree");
    select(&mut p, 1, 2);
    assert_eq!(p.fold_selection(), Err(FoldError::Output));

    let mut depth = pane(40, 1024 * 1024);
    for level in 0..8 {
        add(&mut depth, 2 + level, 38 - level);
    }
    select(&mut depth, 10, 29);
    assert_eq!(depth.fold_selection(), Err(FoldError::Limit));
    assert_eq!(depth.fold_regions().iter().filter(|r| r.manual).count(), 8);
    let mut count = pane(420, 4 * 1024 * 1024);
    for n in 0..128 {
        add(&mut count, 2 + n * 3, 3 + n * 3);
    }
    select(&mut count, 400, 401);
    assert_eq!(count.fold_selection(), Err(FoldError::Limit));
}

#[test]
fn nested_search_and_reading_commands_open_ancestors_but_keep_other_children() {
    let mut p = pane(24, 1024 * 1024);
    let inner = add(&mut p, 4, 6);
    let other = add(&mut p, 13, 15);
    let outer = add(&mut p, 2, 10);
    let root = p.command_blocks()[0].id;
    p.fold_region_set(root, true, true);
    p.search_open();
    p.search_set_query("ROW_004");
    p.search_step(true);
    assert!(!folded(&p, root) && !folded(&p, outer) && !folded(&p, inner));
    assert!(folded(&p, other));
    p.fold_region_jump(inner);
    keys(&mut p, "zc");
    assert!(folded(&p, inner));
    keys(&mut p, "zc");
    assert!(folded(&p, outer));
    keys(&mut p, "zo");
    assert!(!folded(&p, outer) && folded(&p, inner));
    keys(&mut p, "zO");
    assert!(p.fold_regions().iter().all(|r| !r.folded));
    keys(&mut p, "zC");
    assert!(p.fold_regions().iter().all(|r| r.folded));
}

#[test]
fn manual_ids_follow_retained_sources_and_retire_with_their_command() {
    let mut wrapped =
        TerminalPane::with_history(16, 10, Box::new(SoftwareRenderer::new()), 100, 1024 * 1024);
    wrapped.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo WRAP\r\n\x1b]133;C\x07abcdefghijklmnopqrstuvwxyz0123456789ABCD\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let wrapped_root = wrapped.command_blocks()[0].id;
    add(&mut wrapped, 1, 2);
    wrapped.command_set_folded(wrapped_root, true);
    wrapped.resize(128, 10);
    assert!(wrapped.fold_regions().is_empty());
    assert!(
        !wrapped
            .command_blocks()
            .iter()
            .find(|b| b.id == wrapped_root)
            .unwrap()
            .folded,
        "a one-row output after reflow must not retain a phantom folded flag"
    );
    let mut p = pane(30, 8192);
    let child = add(&mut p, 4, 8);
    let root = p.command_blocks()[0].id;
    p.feed("more\r\nmore\r\n");
    assert_eq!(
        p.fold_regions()
            .iter()
            .find(|r| r.id == child)
            .unwrap()
            .parent,
        root
    );
    p.resize(24, 12);
    assert!(p.fold_regions().iter().any(|r| r.id == child));
    p.feed("\x1b[?1049hALT");
    assert!(p.fold_regions().is_empty());
    assert!(!p.fold_region_set(child, false, false));
    p.feed("\x1b[?1049l");
    assert!(p.fold_regions().iter().any(|r| r.id == child));
    for row in 0..1000 {
        p.feed(&format!("new{row:04}\r\n"));
    }
    assert!(!p.fold_regions().iter().any(|r| r.id == child));
    assert!(!p.fold_region_jump(child));
    p.clear();
    assert_eq!(p.fold_region_count(), 0);
}
