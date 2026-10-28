//! Local grid mutations never advance the peer's output parser.
use super::{projection::Projection, TermGrid};
use alacritty_terminal::vte::ansi::{ClearMode, Handler};

impl TermGrid {
    /// Archive the visible primary screen, then erase it and home the cursor.
    pub(crate) fn clear_screen(&mut self) {
        self.term.goto(0, 0);
        self.erase_screen(ClearMode::All);
        self.projection = Projection::default();
        self.set_view_offset(0);
        self.dirty = true;
    }

    /// Preserve parser cursor semantics while giving ED2 room to archive a screen.
    pub(super) fn erase_screen(&mut self, mode: ClearMode) {
        let primary = !self.alt_screen();
        let archive = primary && matches!(mode, ClearMode::All);
        let saved = primary && matches!(mode, ClearMode::Saved);
        let raw_limit = self.live_window.saturating_mul(2);
        if archive {
            self.drain_pending_above(1);
            // ED2 can scroll a whole screen at once, exceeding a small raw window.
            // Give that one mutation room; archive its rows before restoring the cap.
            self.term
                .grid_mut()
                .update_history(raw_limit.max(self.size.rows));
        }
        self.term.clear_screen(mode);
        if archive {
            self.drain_pending_above(1);
            self.flush_pending();
            self.term.grid_mut().update_history(raw_limit);
        }
        if saved {
            self.retire_history();
        }
    }

    /// Retire all active-document history and its projections; keep visible text.
    /// The alternate screen has no history and must not erase the inactive primary.
    pub fn clear_scrollback(&mut self) {
        if self.alt_screen() {
            return;
        }
        self.erase_screen(ClearMode::Saved);
    }

    /// Called only after the parser has erased raw history or reset both grids.
    pub(super) fn retire_history(&mut self) {
        self.pending.clear();
        self.store.clear();
        self.row_cache.get_mut().clear();
        self.projection = Projection::default();
        self.set_view_offset(0);
        self.dirty = true;
    }
}
