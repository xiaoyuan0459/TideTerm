//! Source text extraction; columns keep physical rows and an explicit copy budget.
use super::TerminalPane;
use crate::selection::{Selection, SelectionKind};

const COPY_BYTES: usize = 1024 * 1024;
#[derive(Debug)]
pub enum SelectionCopyError {
    TooLarge,
}

impl TerminalPane {
    pub fn selection_text(&self) -> Option<String> {
        self.selection_text_result().ok().flatten()
    }
    pub fn selection_text_result(&self) -> Result<Option<String>, SelectionCopyError> {
        let mut ranges: Vec<_> = self.live_selections().collect();
        ranges.sort_by_key(|(id, s)| {
            let (a, b) = s.ordered();
            (a.line, a.col, b.line, b.col, *id)
        });
        let mut text = String::new();
        for (_, sel) in ranges {
            let part = if sel.kind == SelectionKind::Column {
                self.column_text(sel)?
            } else {
                self.stream_text(sel)?
            };
            if let Some(part) = part {
                let separator = usize::from(!text.is_empty());
                if text
                    .len()
                    .saturating_add(separator)
                    .saturating_add(part.len())
                    > COPY_BYTES
                {
                    return Err(SelectionCopyError::TooLarge);
                }
                if separator != 0 {
                    text.push('\n');
                }
                text.push_str(&part);
            }
        }
        Ok((!text.is_empty()).then_some(text))
    }
    fn stream_text(&self, sel: &Selection) -> Result<Option<String>, SelectionCopyError> {
        let (start, end) = sel.ordered();
        let (cols, _) = self.grid_size();
        if cols == 0 {
            return Ok(None);
        }
        let last_col = cols - 1;
        // Read straight from the grid by ABSOLUTE line, so a selection anchored in scrollback (or
        // straddling the history/viewport boundary) reconstructs correctly regardless of scroll.
        let mut text = String::new();
        for line_i in start.line..=end.line {
            let col_start = if line_i == start.line { start.col } else { 0 };
            let col_end = if line_i == end.line {
                end.col
            } else {
                last_col
            };
            let Some(line) = self
                .grid
                .selected_cells(line_i, col_start, col_end.min(last_col))
            else {
                continue;
            };
            let selected = if self.grid.line_wraps(line_i) {
                &line
            } else {
                line.trim_end()
            };
            if text.len().saturating_add(selected.len()) > COPY_BYTES {
                return Err(SelectionCopyError::TooLarge);
            }
            text.push_str(selected);
            if line_i < end.line && !self.grid.line_wraps(line_i) {
                text.push('\n');
            }
            if text.len() > COPY_BYTES {
                return Err(SelectionCopyError::TooLarge);
            }
        }
        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }
    fn column_text(&self, sel: &Selection) -> Result<Option<String>, SelectionCopyError> {
        let (start, end) = sel.ordered();
        let lines = (i64::from(end.line) - i64::from(start.line) + 1) as usize;
        let width = end.col - start.col + 1;
        if lines.saturating_mul(width).saturating_add(lines - 1) > COPY_BYTES {
            return Err(SelectionCopyError::TooLarge);
        }
        let mut text = String::new();
        for line in start.line..=end.line {
            if line != start.line {
                text.push('\n');
            }
            let mut next = start.col;
            for cell in self
                .grid
                .reading_cells(line)
                .into_iter()
                .filter(|c| c.col <= end.col && c.col + c.width > start.col)
            {
                text.extend(std::iter::repeat_n(' ', cell.col.saturating_sub(next)));
                text.push_str(&cell.text);
                next = next.max(cell.col + cell.width);
                if text.len() > COPY_BYTES {
                    return Err(SelectionCopyError::TooLarge);
                }
            }
            text.extend(std::iter::repeat_n(' ', (end.col + 1).saturating_sub(next)));
            if text.len() > COPY_BYTES {
                return Err(SelectionCopyError::TooLarge);
            }
        }
        Ok((!text.trim().is_empty()).then_some(text))
    }
}
