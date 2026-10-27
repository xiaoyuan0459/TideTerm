//! Bounded history data crosses cxx; the Qt scrollbar holds only view state.
use super::term::try_with_tab;
use crate::bridge::reading::reading_ffi::{HistoryState, HistoryText};

pub fn term_history_state(idx: u32) -> HistoryState {
    if super::term_ops::input_locked() {
        return HistoryState::default();
    }
    try_with_tab(idx, |st| {
        let (position, maximum, rows) = st.pane.history_scroll_state();
        HistoryState {
            available: true,
            position: position as u32,
            maximum: maximum as u32,
            rows: rows as u32,
            cols: st.pane.grid_size().0 as u32,
            generation: st.generation,
            revision: st.preview_revision,
        }
    })
    .unwrap_or_default()
}

pub fn term_history_preview(idx: u32, position: u32) -> HistoryText {
    if super::term_ops::input_locked() {
        return HistoryText::default();
    }
    try_with_tab(idx, |st| {
        st.pane
            .history_preview(position as usize)
            .map(|(first_line, text, truncated)| HistoryText {
                available: true,
                first_line,
                text,
                truncated,
            })
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn term_history_scroll_to(idx: u32, position: u32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        st.pane.history_scroll_to(position as usize);
        st.redraw_pending = true;
    })
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::{term, term_actions, term_diag, term_ops, term_tests};

    #[test]
    fn history_bridge_revises_in_place_output_and_rejects_locked_or_retired_sources() {
        let _serial = term_tests::input_lock_guard();
        let tab = term::term_new_tab();
        term_diag::term_feed_inject(tab, b"BEFORE");
        let before = term_history_state(tab);
        term_diag::term_feed_inject(tab, b"\rAFTER");
        let after = term_history_state(tab);
        assert_eq!(before.maximum, after.maximum);
        assert_ne!(
            before.revision, after.revision,
            "in-place output must invalidate the snapshot"
        );
        assert!(term_history_preview(tab, 0).text.starts_with("AFTER"));
        term_actions::term_clear(tab, 2);
        assert_ne!(term_history_state(tab).revision, after.revision);
        term_ops::term_set_locked(true);
        let locked = term_history_state(tab);
        let text = term_history_preview(tab, 0);
        let scrolled = term_history_scroll_to(tab, 0);
        term_ops::term_set_locked(false);
        assert!(!locked.available && !text.available && !scrolled);
        term::term_close_tab(tab);
        assert!(!term_history_state(tab).available);
        assert!(!term_history_preview(tab, 0).available);
        assert!(!term_history_scroll_to(tab, 0));
    }
}
