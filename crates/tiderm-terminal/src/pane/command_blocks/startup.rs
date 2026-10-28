//! Opt-in source range before the first shell prompt. No generated terminal text.
use super::{Block, Commands, TerminalPane};
use crate::selection::Cell;

impl TerminalPane {
    /// Hosts opt in once for shell sessions; generic panes and raw protocols stay unchanged.
    pub fn command_startup_begin(&mut self) {
        if !self.grid.alt_screen() && self.commands.blocks.is_empty() {
            self.commands.startup = Some(self.command_cursor());
        }
    }

    pub fn command_startup_pending(&self) -> bool {
        self.commands.startup.is_some()
    }

    /// Called by OSC 133 A or a host's existing, recognized prompt fallback.
    pub fn command_startup_finish(&mut self) -> bool {
        self.commands.startup_finish(&self.grid)
    }
}

impl Commands {
    pub(super) fn startup_finish(&mut self, grid: &crate::grid::TermGrid) -> bool {
        if grid.alt_screen() {
            return false;
        }
        let Some(mut start) = self.startup.take() else {
            return false;
        };
        let mut end = Self::cursor(grid);
        end.col = 0;
        while grid.line_wraps(end.line - 1) {
            end.line -= 1;
        }
        if end.line <= start.line {
            return false;
        }
        // ConPTY may advance over empty initialization rows before the real banner.
        // A disclosure anchored to those offscreen blanks would be unreachable.
        let first = (start.line..end.line).take(1024).find(|&line| {
            grid.reading_cells(line)
                .iter()
                .any(|cell| !cell.text.trim().is_empty())
        });
        let Some(first) = first else {
            return false;
        };
        if first != start.line {
            start = Cell {
                line: first,
                col: 0,
            };
        }
        let (text, _) = Self::range(grid, start, end, 1024, 32);
        if text.trim().is_empty() {
            return false;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            return false;
        };
        self.next_id = id;
        self.blocks.push_front(Block {
            id,
            title: String::new(),
            prompt: start,
            output: start,
            end: Some(Cell { col: 0, ..end }),
            folded: false,
            regions: Vec::new(),
            startup: true,
        });
        true
    }
}
