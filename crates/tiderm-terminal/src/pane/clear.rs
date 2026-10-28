//! Local display actions retire view state without writing into the peer's parser.
use super::TerminalPane;

enum Target {
    Screen,
    History,
    All,
}

impl TerminalPane {
    /// Archive and erase the visible screen, preserving terminal modes and history.
    pub fn clear_screen(&mut self) {
        self.clear_local(Target::Screen);
    }

    /// Erase the active document's history; preserve visible text and pending output.
    pub fn clear_scrollback(&mut self) {
        self.clear_local(Target::History);
    }

    /// Clear the active screen and its history without resetting application modes.
    pub fn clear(&mut self) {
        self.clear_local(Target::All);
    }

    fn clear_local(&mut self, target: Target) {
        let primary = !self.grid.alt_screen();
        if !primary && matches!(target, Target::History) {
            return;
        }
        match target {
            Target::Screen => self.grid.clear_screen(),
            Target::History => self.grid.clear_scrollback(),
            Target::All => {
                self.grid.clear_screen();
                self.grid.clear_scrollback();
            }
        }
        self.retire_view_state(primary, false);
    }

    /// Local actions and executed output mutations share the same retirement.
    pub(super) fn retire_view_state(&mut self, primary: bool, restart_startup: bool) {
        if primary {
            let startup = restart_startup && self.commands.startup.is_some();
            self.commands.invalidate();
            if startup {
                self.command_startup_begin();
            }
        }
        self.retire_user_state();
    }

    pub(super) fn retire_user_state(&mut self) {
        self.navigation.invalidate();
        self.sync_command_folds();
        self.selection_clear();
        if self.reading.active {
            self.reading_cancel();
            self.reading_reset_cursor();
        }
        self.search_recompute();
    }
}
