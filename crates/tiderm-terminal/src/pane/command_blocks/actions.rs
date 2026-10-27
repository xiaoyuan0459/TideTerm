//! Context actions use retained source coordinates and stable command identities.
use super::super::TerminalPane;
use crate::selection::Cell;

impl TerminalPane {
    pub fn command_at_point(&self, x: f32, y: f32, width: f32, height: f32) -> Option<u64> {
        if self.grid.alt_screen() {
            return None;
        }
        let cell = self.cell_at_clamped(x, y, width, height)?;
        self.commands
            .blocks
            .iter()
            .rev()
            .find(|block| {
                let end = block.end.unwrap_or_else(|| self.command_cursor());
                !block.startup
                    && (cell.line, cell.col) >= (block.prompt.line, block.prompt.col)
                    && (cell.line, cell.col) < (end.line, end.col)
            })
            .map(|block| block.id)
    }

    /// Select the command block including its prompt and captured output.
    /// The next prompt and other independent selections are excluded.
    pub fn command_select_block(&mut self, id: u64) -> bool {
        if self.selection_locked || self.grid.alt_screen() {
            return false;
        }
        let Some(block) = self
            .commands
            .blocks
            .iter()
            .find(|block| block.id == id && !block.startup)
        else {
            return false;
        };
        let start = block.prompt;
        let end = block.end.unwrap_or_else(|| self.command_cursor());
        if (end.line, end.col) <= (start.line, start.col) {
            return false;
        }
        let head = if end.col == 0 {
            Cell {
                line: end.line - 1,
                col: self.grid.size().cols.saturating_sub(1),
            }
        } else {
            Cell {
                col: end.col - 1,
                ..end
            }
        };
        self.fold_region_set(id, false, true);
        self.selection_clear();
        self.reading.visual_column = false;
        self.reading.visual = Some(start);
        self.reading_move(head);
        true
    }
}
