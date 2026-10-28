//! Cell metadata for local reading, retaining terminal columns and graphemes.
use super::{Column, Flags, Line, TermGrid};
use alacritty_terminal::grid::Dimensions;

#[derive(Clone, Debug)]
pub(crate) struct ReadCell {
    pub col: usize,
    pub text: String,
    pub width: usize,
}

impl TermGrid {
    pub(crate) fn reading_rows(&self) -> (i32, i32) {
        let history = self.total_history();
        (
            -(history.min(i32::MAX as usize) as i32),
            self.size.rows as i32 - 1,
        )
    }

    pub(crate) fn reading_cells(&self, line: i32) -> Vec<ReadCell> {
        let (first, last) = self.reading_rows();
        if !(first..=last).contains(&line) {
            return Vec::new();
        }
        let history = self.term.grid().history_size() as i32;
        let mut cells = Vec::new();
        if line >= -history {
            let row = &self.term.grid()[Line(line)];
            for col in 0..self.size.cols {
                let cell = &row[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let mut text = if cell.c == '\0' {
                    " ".into()
                } else {
                    cell.c.to_string()
                };
                if let Some(marks) = cell.zerowidth() {
                    text.extend(marks);
                }
                cells.push(ReadCell {
                    col,
                    text,
                    width: if cell.flags.contains(Flags::WIDE_CHAR) {
                        2
                    } else {
                        1
                    },
                });
            }
        } else if let Some(row) = self
            .store
            .row((self.stored_rows() as i32 + history + line) as usize)
        {
            // Trailing blank columns are not stored; `cols()` is the row's real width, and a
            // column past the stored chars is a plain space (same cell the full-width row had).
            for col in 0..row.cols().min(self.size.cols) {
                if row
                    .runs
                    .iter()
                    .any(|r| r.wide && col == r.start as usize + 1)
                {
                    continue;
                }
                let text = row
                    .extras
                    .iter()
                    .find(|(c, _)| *c as usize == col)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_else(|| {
                        let ch = row.chars.get(col).copied().unwrap_or(' ' as u16);
                        char::from_u32(ch as u32).unwrap_or(' ').to_string()
                    });
                if text.is_empty() {
                    continue;
                }
                let width = if row.runs.iter().any(|r| r.wide && col == r.start as usize) {
                    2
                } else {
                    1
                };
                cells.push(ReadCell { col, text, width });
            }
        }
        cells
    }
}
