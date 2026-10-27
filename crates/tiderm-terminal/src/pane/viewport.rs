//! Viewport policy selected by the host, without stopping incoming output.
use super::TerminalPane;
#[path = "history_preview.rs"]
mod history_preview;

impl TerminalPane {
    /// Remap bounded anchors through raw reflow; archived coordinates only shift.
    pub fn resize(&mut self, cols: usize, rows: usize) -> bool {
        if cols.max(2) == self.grid.size().cols && rows.max(1) == self.grid.size().rows {
            return false;
        }
        if cols.max(2) < self.grid.size().cols {
            self.grid.prepare_column_resize();
        }
        let keep_columns = cols.max(2) == self.grid.size().cols;
        let before = self.grid.reflow_layout();
        let changed = self.grid.resize(cols, rows);
        let after = self.grid.reflow_layout();
        let first = self.grid.reading_rows().0;
        let map = |cell| before.map(cell, &after, first);
        if self.grid.alt_screen() {
            self.navigation.invalidate();
            // The inactive main grid is not the grid represented by this layout.
            self.commands.invalidate();
            self.selection_clear();
            self.reading_cancel();
        } else {
            self.commands.remap(map);
            self.navigation.remap(&map);
            let active_lost = self.remap_selections(map, keep_columns);
            self.sync_command_folds();
            if self.reading.active {
                self.reading.cursor = map(self.reading.cursor).unwrap_or(crate::selection::Cell {
                    line: first,
                    col: 0,
                });
                self.reading.visual = self.reading.visual.and_then(map);
                self.reading.operator = self
                    .reading
                    .operator
                    .and_then(|(cell, count)| map(cell).map(|cell| (cell, count)));
                self.reading_clamp(false);
            }
            if active_lost {
                self.sync_active_reading();
            }
        }
        self.sync_command_folds();
        changed
    }

    pub fn set_follow_output(&mut self, follow: bool) {
        self.grid.set_follow_output(follow);
    }

    pub(super) fn feed_viewport(&mut self, bytes: &[u8]) {
        let alternate = self.grid.alt_screen();
        let commands = &mut self.commands;
        let result = self
            .grid
            .feed_observed(bytes, &mut |grid, event| commands.observe(grid, event));
        if !self.grid.alt_screen() {
            self.commands.shift(0, self.grid.reading_rows().0);
        }
        if result.clear_view {
            self.retire_user_state();
            return;
        }
        let scrolled = result.scrolled;
        if !alternate && !self.grid.alt_screen() {
            // Reading clamps below must use the same shifted source ranges.
            if scrolled != 0 {
                self.sync_command_folds();
            }
        }
        let mut active_lost = false;
        if alternate != self.grid.alt_screen() {
            self.commands.startup = None;
            self.navigation.invalidate();
            self.selection_clear();
            if self.reading.active {
                self.reading_cancel();
                self.reading_reset_cursor();
            }
        } else {
            let shift = i32::try_from(scrolled).unwrap_or(i32::MAX);
            let earliest = -(self.grid.total_history().min(i32::MAX as usize) as i32);
            self.navigation.remap(&|cell| {
                let line = cell.line.saturating_sub(shift);
                (line >= earliest).then_some(crate::selection::Cell { line, ..cell })
            });
            active_lost = self.remap_selections(
                |cell| {
                    let line = cell.line.saturating_sub(shift);
                    (line >= earliest).then_some(crate::selection::Cell { line, ..cell })
                },
                true,
            );
        }
        if self.reading.active && alternate == self.grid.alt_screen() {
            self.reading.cursor.line = self
                .reading
                .cursor
                .line
                .saturating_sub(i32::try_from(scrolled).unwrap_or(i32::MAX));
            if let Some(anchor) = self.reading.visual.as_mut() {
                anchor.line = anchor
                    .line
                    .saturating_sub(i32::try_from(scrolled).unwrap_or(i32::MAX));
            }
            if self.reading.cursor.line < self.grid.reading_rows().0 {
                self.reading_cancel();
            }
            self.reading_clamp(false);
        }
        if active_lost {
            self.sync_active_reading();
        }
        self.sync_command_folds();
    }
}
