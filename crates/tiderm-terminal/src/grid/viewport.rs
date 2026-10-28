//! One owner for viewport intent; the parser remains the screen-mode authority.
use super::TermGrid;
use alacritty_terminal::grid::Dimensions;

pub(super) struct Viewport {
    offset: usize,
    follow_output: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: 0,
            follow_output: true,
        }
    }
}

impl Viewport {
    pub(super) fn offset(&self) -> usize {
        self.offset
    }

    pub(super) fn is_held(&self) -> bool {
        !self.follow_output || self.offset != 0
    }

    pub(super) fn move_to(&mut self, requested: usize, history: usize) -> bool {
        let offset = requested.min(history);
        let changed = offset != self.offset;
        self.offset = offset;
        changed
    }

    pub(super) fn after_output(&self, growth: usize, primary: bool) -> usize {
        if primary && self.is_held() {
            self.offset.saturating_add(growth)
        } else {
            0
        }
    }
}

impl TermGrid {
    /// Retained primary rows belong to the primary document even while a TUI runs.
    pub(super) fn stored_rows(&self) -> usize {
        if self.alt_screen() {
            0
        } else {
            self.store.rows()
        }
    }

    pub(crate) fn reflow_boundary(&self) -> i32 {
        -(self.term.grid().history_size().min(i32::MAX as usize) as i32)
    }

    /// Freeze raw history at its original width before reflowing live rows.
    /// Direct grid operations leave fragmented UTF-8/OSC/CSI parser state intact.
    pub(crate) fn prepare_column_resize(&mut self) {
        self.flush_pending();
        self.drain_pending_above(1);
        self.flush_pending();
    }

    pub fn set_follow_output(&mut self, follow: bool) {
        self.viewport.follow_output = follow;
    }
}
