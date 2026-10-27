//! Scrollbar positions use the same folded source projection as the viewport.
use super::TerminalPane;

impl TerminalPane {
    pub fn history_snapshot(&self) -> Option<crate::history_snapshot::HistorySnapshot> {
        self.grid.history_snapshot()
    }

    /// `(position, maximum, page_rows)`, oldest at zero and live output at maximum.
    pub fn history_scroll_state(&self) -> (usize, usize, usize) {
        let maximum = self.grid.visible_index(0).max(0) as usize;
        let position = self
            .grid
            .visible_index(-(self.grid.display_offset() as i32))
            .clamp(0, maximum as i32) as usize;
        (position, maximum, self.grid.size().rows)
    }

    pub fn history_scroll_to(&mut self, position: usize) {
        let (current, maximum, _) = self.history_scroll_state();
        self.scroll_by(current as i32 - position.min(maximum) as i32);
    }

    /// Read at most 32 physical visible rows / 8 KiB without moving or unfolding anything.
    pub fn history_preview(&self, position: usize) -> Option<(u64, String, bool)> {
        let (_, maximum, _) = self.history_scroll_state();
        if self.grid.alt_screen() || position > maximum {
            return None;
        }
        let (_, last) = self.grid.reading_rows();
        let first = self.grid.source_index(position as i32);
        let mut text = String::new();
        for row in 0..32 {
            let line = self.grid.source_index(position as i32 + row);
            if line > last {
                return Some((self.history_line_number(first), text, false));
            }
            let mut value: String = self
                .grid
                .reading_cells(line)
                .into_iter()
                .map(|cell| cell.text)
                .collect();
            value.truncate(value.trim_end().len());
            if self.grid.fold_marker(line).is_some() {
                value.push_str(" …");
            }
            if row != 0 {
                value.insert(0, '\n');
            }
            let remaining = (8 * 1024usize).saturating_sub(text.len());
            let mut end = value.len().min(remaining);
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&value[..end]);
            if end < value.len() {
                return Some((self.history_line_number(first), text, true));
            }
        }
        Some((
            self.history_line_number(first),
            text,
            self.grid.source_index(position as i32 + 32) <= last,
        ))
    }

    fn history_line_number(&self, line: i32) -> u64 {
        (self.grid.total_history() as i64 + i64::from(line) + 1).max(1) as u64
    }
}
