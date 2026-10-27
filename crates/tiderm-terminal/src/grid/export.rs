//! Capture physical source rows independently of viewport, folding and selection.
use super::*;
use std::sync::Arc;

impl TermGrid {
    pub fn history_snapshot(&self) -> Option<crate::history_snapshot::HistorySnapshot> {
        if self.alt_screen() {
            return None;
        }
        let mut rows = self.store.snapshot();
        rows.extend(self.pending.iter().cloned().map(Arc::new));
        let grid = self.term.grid();
        let history = grid.history_size() as i32;
        for index in -history..self.size.rows as i32 {
            rows.push(Arc::new(
                self.line_data_from_grid_row(&grid[Line(index)], self.size.cols),
            ));
        }
        Some(crate::history_snapshot::HistorySnapshot { rows })
    }
}
