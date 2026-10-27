//! Rule matching, visual precedence and source lifetime belong to the pane API.
use tiderm_terminal::{
    marks::{MarkError, MarkSpec, MarkedCells},
    search::SearchOptions,
    SoftwareRenderer, TerminalPane,
};
fn pane(cols: usize, rows: usize) -> TerminalPane {
    TerminalPane::with_history(cols, rows, Box::new(SoftwareRenderer::new()), 2, 32 * 1024)
}
fn spec(pattern: &str, color: u8, flags: u32) -> MarkSpec {
    MarkSpec {
        pattern: pattern.into(),
        options: SearchOptions::from_flags(flags),
        color,
        enabled: true,
    }
}
fn keys(p: &mut TerminalPane, text: &str) {
    for c in text.chars() {
        p.reading_key(0, 0, &c.to_string());
    }
}

#[test]
fn independent_rules_keep_search_selection_and_last_rule_priority_separate() {
    let mut p = pane(40, 8);
    p.feed("ERROR WARN ERROR\r\n");
    let base = p.mark_put(0, spec("ERROR", 0, 1)).unwrap();
    let overlap = p.mark_put(0, spec("ERR", 2, 1)).unwrap();
    assert_eq!(
        p.mark_cells()[0],
        MarkedCells {
            row: 0,
            start: 0,
            end: 3,
            color: 2
        }
    );
    assert_eq!(
        p.mark_cells()[1],
        MarkedCells {
            row: 0,
            start: 3,
            end: 5,
            color: 0
        }
    );
    assert!(p.mark_move(overlap, true));
    assert_eq!(
        p.mark_cells()[0],
        MarkedCells {
            row: 0,
            start: 0,
            end: 5,
            color: 0
        }
    );
    let before = p.mark_cells();
    p.search_open();
    p.search_set_query("WARN");
    p.search_close();
    assert_eq!(p.mark_cells(), before);
    p.set_reading_mode(true);
    keys(&mut p, "ggv4l");
    assert_eq!(p.mark_seed(true).unwrap(), "ERROR");
    p.selection_clear();
    assert_eq!(p.mark_cells(), before);
    let mut disabled = spec("ERROR", 4, 1);
    disabled.enabled = false;
    p.mark_put(base, disabled).unwrap();
    assert!(p.mark_cells().iter().all(|r| r.color == 2));
    assert!(p.mark_remove(overlap));
    assert!(p.mark_cells().is_empty());
    assert_eq!(p.mark_quick(5, false).unwrap(), base);
    assert_eq!(p.mark_rules().len(), 1);
    assert!(p.mark_cells().iter().all(|r| r.color == 5));
    p.mark_quick(0, true).unwrap();
    assert!(p.mark_rules().is_empty());
    assert!(p.screen_text().contains("ERROR WARN ERROR"));
}

#[test]
fn unicode_graphemes_case_expansion_and_selection_search_use_terminal_columns() {
    let mut p = pane(40, 8);
    p.feed("İ中e\u{301} WARN warn WARNx\r\nİ中e\u{301} WARN\r\n");
    p.mark_put(0, spec("i\u{307}中e\u{301}", 1, 0)).unwrap();
    assert_eq!(
        p.mark_cells()[0],
        MarkedCells {
            row: 0,
            start: 0,
            end: 4,
            color: 1
        }
    );
    p.mark_put(0, spec("WARN", 3, 4)).unwrap();
    assert_eq!(p.mark_cells().iter().filter(|m| m.color == 3).count(), 3);
    p.set_reading_mode(true);
    keys(&mut p, "gg4lv3l");
    assert_eq!(p.mark_seed(true).unwrap(), "WARN");
    p.search_set_options(SearchOptions::from_flags(3));
    p.search_selection(true).unwrap();
    assert_eq!(p.search_query(), "WARN");
    assert_eq!(p.search_options().to_flags(), 1);
    assert_eq!(p.search_count(), (2, 3));
    p.search_selection(false).unwrap();
    assert_eq!(p.search_count(), (3, 3));
    p.selection_clear();
    keys(&mut p, "gg5l");
    assert_eq!(p.mark_seed(false).unwrap(), "WARN");
    p.reading_key(0, 4, "\u{16}");
    keys(&mut p, "2l");
    assert_eq!(p.mark_seed(true), Err(MarkError::Selection));
}

#[test]
fn dynamic_rules_follow_compressed_reflowed_folded_and_retired_content() {
    let mut p = pane(24, 6);
    p.feed("\x1b]133;A\x07$ \x1b]133;B\x07echo MARK\r\n\x1b]133;C\x07");
    for n in 0..25 {
        p.feed(&format!("EVENT_{n:02} 中\r\n"));
    }
    p.feed("\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let rule = p.mark_put(0, spec("EVENT_04", 4, 1)).unwrap();
    let root = p.command_blocks()[0].id;
    p.command_set_folded(root, true);
    p.history_scroll_to(0);
    assert!(p.mark_cells().is_empty());
    p.set_reading_mode(true);
    p.mark_step(rule, true).unwrap();
    assert!(!p.command_blocks()[0].folded);
    assert_eq!(p.mark_cells().len(), 1);
    let search = (p.search_query().to_owned(), p.search_count());
    p.resize(12, 8);
    p.mark_step(rule, true).unwrap();
    assert_eq!(p.mark_cells().len(), 1);
    assert_eq!((p.search_query().to_owned(), p.search_count()), search);
    p.feed("\x1b[?1049hEVENT_04");
    assert!(p.mark_cells().is_empty());
    assert_eq!(p.mark_step(rule, true), Err(MarkError::NoMatch));
    p.feed("\x1b[?1049l");
    p.mark_step(rule, true).unwrap();
    assert_eq!(p.mark_cells().len(), 1);
    for n in 0..1500 {
        p.feed(&format!("replacement {n}\r\n"));
    }
    assert_eq!(p.mark_step(rule, true), Err(MarkError::NoMatch));
    assert_eq!(p.mark_rules().len(), 1);
    p.clear();
    p.feed("EVENT_04\r\n");
    assert_eq!(p.mark_cells().len(), 1);
}

#[test]
fn validation_is_atomic_and_render_work_and_rule_state_are_bounded() {
    let mut p = pane(8200, 2);
    p.feed(&"AB".repeat(4100));
    let id = p.mark_put(0, spec("A", 0, 1)).unwrap();
    assert_eq!(p.mark_cells().len(), 4096);
    assert_eq!(p.mark_put(id, spec("(", 0, 2)), Err(MarkError::Regex));
    assert_eq!(p.mark_put(0, spec("A", 2, 1)), Err(MarkError::Duplicate));
    assert_eq!(p.mark_put(0, spec("x", 6, 1)), Err(MarkError::Color));
    assert_eq!(
        p.mark_put(0, spec(&"x".repeat(513), 0, 1)),
        Err(MarkError::Pattern)
    );
    for n in 1..16 {
        p.mark_put(0, spec(&format!("r{n}"), 0, 1)).unwrap();
    }
    assert_eq!(p.mark_put(0, spec("extra", 0, 1)), Err(MarkError::Limit));
    let before = p.mark_rules();
    assert_eq!(
        p.mark_replace(&[spec("valid", 0, 1), spec("(", 0, 2)]),
        Err(MarkError::Regex)
    );
    assert_eq!(p.mark_rules(), before);
    p.mark_replace(&[spec("B", 2, 1)]).unwrap();
    assert!(p.mark_rules()[0].id > id);
    assert!(!p.mark_remove(id));
    p.mark_clear();
    assert!(p.mark_cells().is_empty());
}

#[test]
fn zero_width_regex_has_no_painted_cells_in_search_or_independent_marks() {
    let mut p = pane(16, 4);
    p.feed("word\r\n");
    p.search_open();
    p.search_set_options(SearchOptions::from_flags(2));
    p.search_set_query(r"\b");
    assert_eq!(
        p.search_count(),
        (0, 0),
        "zero-width regex must not invent a first-cell hit"
    );
    p.mark_put(0, spec(r"\b", 0, 2)).unwrap();
    assert!(p.mark_cells().is_empty());
}
