//! Read-only terminal metadata, independent of viewport rendering.
use super::TermGrid;
use alacritty_terminal::grid::Dimensions;

impl TermGrid {
    /// OSC boundaries are after the final printed cell even while wrap is pending.
    pub(crate) fn cursor_boundary_col(&self) -> usize {
        let cursor = &self.term.grid().cursor;
        cursor.point.column.0 + usize::from(cursor.input_needs_wrap)
    }
    /// Selection ranges use cell columns, but must not copy wide-character spacers.
    pub(crate) fn selected_cells(&self, line: i32, mut start: usize, end: usize) -> Option<String> {
        use super::{Column, Flags, Line};
        let history = self.term.grid().history_size() as i32;
        if line < -(self.total_history() as i32) || line >= self.size.rows as i32 {
            return None;
        }
        let mut text = String::new();
        if line >= -history {
            start = start.min(self.size.cols - 1);
            let row = &self.term.grid()[Line(line)];
            if start > 0 && row[Column(start)].flags.contains(Flags::WIDE_CHAR_SPACER) {
                start -= 1;
            }
            for col in start..=end.min(self.size.cols - 1) {
                let cell = &row[Column(col)];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    text.push(if cell.c == '\0' { ' ' } else { cell.c });
                    if let Some(marks) = cell.zerowidth() {
                        text.extend(marks);
                    }
                }
            }
        } else {
            let row = self
                .store
                .row((self.stored_rows() as i32 + history + line) as usize)?;
            let spacer = |col| {
                row.runs
                    .iter()
                    .any(|run| run.wide && col == run.start as usize + 1)
            };
            start = start.min(row.cols().saturating_sub(1));
            if start > 0 && spacer(start) {
                start -= 1;
            }
            // Trailing blank columns are not stored; `cols()` is the row's real width, so the
            // range matches the live-row path and missing columns read as spaces.
            for col in start..=end.min(row.cols().saturating_sub(1)) {
                if !spacer(col) {
                    if let Some((_, full)) = row.extras.iter().find(|(c, _)| *c as usize == col) {
                        text.push_str(full);
                    } else {
                        let ch = row.chars.get(col).copied().unwrap_or(' ' as u16);
                        text.push(char::from_u32(ch as u32).unwrap_or(' '));
                    }
                }
            }
        }
        Some(text)
    }
    /// Total lines retained in the buffer (compressed store + alacritty history +
    /// live viewport rows) — the gutter's 1-based numbering domain.
    pub fn buffer_lines(&self) -> u64 {
        (self.stored_rows() + self.term.grid().history_size() + self.size.rows) as u64
    }
    /// Output metadata uses the live cursor, independent of the scrolled viewport.
    pub fn output_line_number(&self) -> u64 {
        (self.stored_rows() + self.term.grid().history_size()) as u64
            + self.term.grid().cursor.point.line.0.max(0) as u64
            + 1
    }
}
