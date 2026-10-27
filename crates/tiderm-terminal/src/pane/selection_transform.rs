//! Explicit bounded selection edits; terminal text is never modified.
use super::multi_selection::{SavedSelection, MAX_SELECTIONS};
use super::TerminalPane;
use crate::selection::{Cell, Selection, SelectionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SelectionEditError {
    Empty = 1,
    Locked = 2,
    Limit = 3,
    Unavailable = 4,
}

fn range(start: Cell, end: Cell) -> Selection {
    Selection {
        anchor: start,
        head: end,
        dragged: true,
        kind: SelectionKind::Stream,
    }
}

impl TerminalPane {
    pub fn selection_locked(&self) -> bool {
        self.selection_locked
    }

    pub fn set_selection_locked(&mut self, locked: bool) -> bool {
        if locked && self.selection_count() == 0 {
            return false;
        }
        self.end_selection_drag();
        self.reading_cancel();
        self.selection_locked = locked;
        true
    }

    /// Turn the active range into one independently editable range per physical row.
    /// Preflight the complete result so an oversized edit cannot discard old ranges.
    pub fn selection_split(&mut self) -> Result<(), SelectionEditError> {
        if self.selection_locked {
            return Err(SelectionEditError::Locked);
        }
        let selected = self
            .selection
            .filter(|s| s.dragged)
            .ok_or(SelectionEditError::Empty)?;
        let (a, b) = selected.ordered();
        let rows = i64::from(b.line) - i64::from(a.line) + 1;
        if rows <= 1 {
            return Err(SelectionEditError::Empty);
        }
        if rows as usize + self.selection_count() - 1 > MAX_SELECTIONS {
            return Err(SelectionEditError::Limit);
        }
        let (cols, _) = self.grid_size();
        let (first, last) = self.grid.reading_rows();
        if cols.saturating_mul(rows as usize) > 256 * 1024 {
            return Err(SelectionEditError::Limit);
        }
        if cols == 0 || a.line < first || b.line > last {
            return Err(SelectionEditError::Unavailable);
        }
        let mut parts = Vec::new();
        for line in a.line..=b.line {
            let start = if selected.kind == SelectionKind::Column || line == a.line {
                a.col
            } else {
                0
            };
            let end = if selected.kind == SelectionKind::Column || line == b.line {
                b.col
            } else {
                cols - 1
            };
            let cells = self.grid.reading_cells(line);
            let start = cells
                .iter()
                .find(|c| c.col <= start && c.col + c.width > start)
                .map_or(start, |c| c.col);
            let end = self.widen_head(Cell { line, col: start }, Cell { line, col: end });
            let mut part = range(Cell { line, col: start }, end);
            part.kind = selected.kind;
            parts.push(part);
        }
        self.end_selection_drag();
        self.reading_cancel();
        self.selection = Some(parts.remove(0));
        for part in parts {
            self.selection_serial = self.selection_serial.saturating_add(1);
            self.saved_selections.push(SavedSelection {
                id: self.selection_serial,
                range: part,
            });
        }
        self.sync_active_reading();
        Ok(())
    }

    /// Complement the union within visible, retained text rows, at whole-glyph boundaries.
    /// Fold placeholder rows and trailing blank padding are outside this visible-text scope.
    pub fn selection_invert(&mut self) -> Result<(), SelectionEditError> {
        if self.selection_locked {
            return Err(SelectionEditError::Locked);
        }
        let (cols, rows) = self.grid_size();
        if cols.saturating_mul(rows) > 256 * 1024 {
            return Err(SelectionEditError::Limit);
        }
        let selected: Vec<_> = self.live_selections().map(|(_, s)| *s).collect();
        let mut parts = Vec::new();
        for row in 0..rows {
            let Some(line) = self.grid.source_row(row) else {
                continue;
            };
            if self.grid.fold_marker(line).is_some() {
                continue;
            }
            let cells = self.grid.reading_cells(line);
            let Some(last) = cells
                .iter()
                .rposition(|c| !c.text.chars().all(char::is_whitespace))
            else {
                continue;
            };
            let mut start = None;
            let mut end = 0;
            for c in &cells[..=last] {
                let covered = selected.iter().any(|s| {
                    let (a, b) = s.ordered();
                    line >= a.line
                        && line <= b.line
                        && if s.kind == SelectionKind::Column {
                            c.col <= b.col && c.col + c.width > a.col
                        } else {
                            (line > a.line || c.col + c.width > a.col)
                                && (line < b.line || c.col <= b.col)
                        }
                });
                if covered {
                    if let Some(col) = start.take() {
                        parts.push(range(Cell { line, col }, Cell { line, col: end }));
                    }
                } else {
                    start.get_or_insert(c.col);
                    end = c.col + c.width - 1;
                }
                if parts.len() > MAX_SELECTIONS {
                    return Err(SelectionEditError::Limit);
                }
            }
            if let Some(col) = start {
                parts.push(range(Cell { line, col }, Cell { line, col: end }));
            }
            if parts.len() > MAX_SELECTIONS {
                return Err(SelectionEditError::Limit);
            }
        }
        self.selection_clear();
        for part in parts {
            if let Some(previous) = self.selection.take() {
                self.saved_selections.push(SavedSelection {
                    id: self.selection_id,
                    range: previous,
                });
            }
            self.next_selection_id();
            self.selection = Some(part);
        }
        self.sync_active_reading();
        Ok(())
    }

    /// Double-click selection is bounded to one physical row; it never resolves a path.
    pub fn selection_token_at(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        path_components: bool,
    ) -> bool {
        if self.selection_locked {
            return false;
        }
        let Some(cell) = self.cell_at_clamped(x, y, w, h) else {
            return false;
        };
        self.unfold_target(cell);
        let cells = self.grid.reading_cells(cell.line);
        let Some(at) = cells
            .iter()
            .position(|c| c.col <= cell.col && c.col + c.width > cell.col)
        else {
            return false;
        };
        let delimiter = |text: &str| {
            text.chars().any(|c| {
                c.is_whitespace()
                    || "\"'()[]{}<>|,;".contains(c)
                    || (path_components && "/\\".contains(c))
            })
        };
        let (mut first, mut last) = (at, at);
        if !delimiter(&cells[at].text) {
            while first > 0 && !delimiter(&cells[first - 1].text) {
                first -= 1;
            }
            while last + 1 < cells.len() && !delimiter(&cells[last + 1].text) {
                last += 1;
            }
        }
        let selected = range(
            Cell {
                line: cell.line,
                col: cells[first].col,
            },
            Cell {
                line: cell.line,
                col: cells[last].col + cells[last].width - 1,
            },
        );
        self.selection_clear();
        self.next_selection_id();
        self.selection = Some(selected);
        self.sync_active_reading();
        true
    }
}
