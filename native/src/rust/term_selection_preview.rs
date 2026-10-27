//! A preview is a bounded display snapshot, separate from full-selection copy.
use crate::bridge::selection::selection_ffi::SelectionPreview;

pub fn term_selection_preview(idx: u32) -> SelectionPreview {
    if super::super::term_ops::input_locked() {
        return SelectionPreview::default();
    }
    let selected = super::term_selection_text(idx);
    if selected.exceeds_budget {
        return SelectionPreview {
            exceeds_budget: true,
            ..SelectionPreview::default()
        };
    }
    let mut text = selected.text;
    let available = !text.is_empty();
    let mut end = text.len().min(64 * 1024);
    if let Some((at, _)) = text.match_indices('\n').nth(1023) {
        end = end.min(at);
    }
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    text.truncate(end);
    SelectionPreview {
        available,
        text,
        truncated,
        exceeds_budget: false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::term::{term_close_tab, term_new_tab};
    use super::*;
    use crate::rust::{term, term_actions, term_diag, term_ops, term_reading, term_tests};

    fn select_history(tab: u32) {
        term_reading::term_set_reading(tab, true);
        for ch in "ggvG$".chars() {
            term::term_key(tab, 0, 0, &ch.to_string());
        }
    }

    #[test]
    fn selection_preview_bounds_utf8_and_lines_at_the_bridge() {
        let _serial = term_tests::input_lock_guard();
        let tab = term_new_tab();
        let input = "a".repeat(65535) + "中tail";
        term_diag::term_feed_inject(tab, input.as_bytes());
        select_history(tab);
        let snapshot = term_selection_preview(tab);
        assert!(snapshot.available && snapshot.truncated && !snapshot.exceeds_budget);
        assert_eq!(snapshot.text, "a".repeat(65535));

        term_reading::term_set_reading(tab, false);
        term_actions::term_clear(tab, 2);
        term_diag::term_feed_inject(tab, "line\r\n".repeat(1100).as_bytes());
        select_history(tab);
        let snapshot = term_selection_preview(tab);
        assert!(snapshot.available && snapshot.truncated);
        assert_eq!(snapshot.text, "line\n".repeat(1023) + "line");
        term_close_tab(tab);
    }

    #[test]
    fn selection_preview_rejects_locked_empty_and_retired_sources() {
        let _serial = term_tests::input_lock_guard();
        let tab = term_new_tab();
        assert!(!term_selection_preview(tab).available);
        term_diag::term_feed_inject(tab, b"preview");
        term_ops::term_select_all(tab);
        assert_eq!(term_selection_preview(tab).text.trim_end(), "preview");
        term_ops::term_set_locked(true);
        let locked = term_selection_preview(tab);
        term_ops::term_set_locked(false);
        assert!(!locked.available && locked.text.is_empty());
        term_close_tab(tab);
        assert!(!term_selection_preview(tab).available);
    }
}
