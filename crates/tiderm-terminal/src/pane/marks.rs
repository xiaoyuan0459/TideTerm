use super::TerminalPane;
use crate::marks::{MarkError, MarkRule, MarkSpec, MarkedCells};
use crate::selection::{Cell, SelectionKind};

impl TerminalPane {
    pub fn mark_rules(&self) -> Vec<MarkRule> {
        self.marks.rules.iter().map(|r| r.row.clone()).collect()
    }
    pub fn mark_put(&mut self, id: u64, spec: MarkSpec) -> Result<u64, MarkError> {
        self.marks.put(id, spec)
    }
    pub fn mark_replace(&mut self, specs: &[MarkSpec]) -> Result<(), MarkError> {
        self.marks.replace(specs)
    }
    pub fn mark_remove(&mut self, id: u64) -> bool {
        let before = self.marks.rules.len();
        self.marks.rules.retain(|r| r.row.id != id);
        before != self.marks.rules.len()
    }
    pub fn mark_clear(&mut self) {
        self.marks.rules.clear();
    }
    pub fn mark_move(&mut self, id: u64, up: bool) -> bool {
        let Some(at) = self.marks.rules.iter().position(|r| r.row.id == id) else {
            return false;
        };
        let next = if up {
            at.checked_sub(1)
        } else {
            at.checked_add(1).filter(|&n| n < self.marks.rules.len())
        };
        if let Some(next) = next {
            self.marks.rules.swap(at, next);
            true
        } else {
            false
        }
    }

    /// Visible source rows only; no full-history result cache or retained positions.
    pub fn mark_cells(&self) -> Vec<MarkedCells> {
        if self.grid.alt_screen() || !self.marks.rules.iter().any(|r| r.row.spec.enabled) {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut budget = 128 * 1024usize;
        for row in 0..self.grid.size().rows.min(512) {
            let Some(line) = self.grid.source_row(row) else {
                break;
            };
            if self.grid.fold_marker(line).is_some() {
                continue;
            }
            let mut used = 0usize;
            let cells: Vec<_> = self
                .grid
                .reading_cells(line)
                .into_iter()
                .take_while(|c| {
                    used += c.text.len();
                    used <= 8192.min(budget)
                })
                .collect();
            budget = budget.saturating_sub(used);
            let mut colors = vec![None; self.grid.size().cols];
            for rule in self.marks.rules.iter().filter(|r| r.row.spec.enabled) {
                for found in rule.matcher.row(&cells, line) {
                    for color in colors.iter_mut().take(found.end).skip(found.start) {
                        *color = Some(rule.row.spec.color);
                    }
                }
            }
            let mut col = 0;
            while col < colors.len() {
                let color = colors[col];
                let start = col;
                while col < colors.len() && colors[col] == color {
                    col += 1;
                }
                if let Some(color) = color {
                    result.push(MarkedCells {
                        row,
                        start,
                        end: col,
                        color,
                    });
                    if result.len() == 4096 {
                        return result;
                    }
                }
            }
            if budget == 0 {
                break;
            }
        }
        result
    }

    pub fn mark_seed(&self, selection_only: bool) -> Result<String, MarkError> {
        if self.grid.alt_screen() {
            return Err(MarkError::Selection);
        }
        let value = if let Some(selected) = self.selection.filter(|s| s.dragged) {
            let (first, last) = selected.ordered();
            if self.selection_count() != 1
                || selected.kind != SelectionKind::Stream
                || first.line != last.line
            {
                return Err(MarkError::Selection);
            }
            self.selection_text().ok_or(MarkError::Selection)?
        } else {
            if selection_only {
                return Err(MarkError::Selection);
            }
            let at = if self.reading.active {
                self.reading.cursor
            } else {
                Cell {
                    line: (self.grid.output_line_number() as i64
                        - self.grid.total_history() as i64
                        - 1) as i32,
                    col: self.grid.cursor_col(),
                }
            };
            let cells = self.grid.reading_cells(at.line);
            let word = |i: usize| {
                cells.get(i).is_some_and(|c| {
                    c.text
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
            };
            let mut pos = cells
                .iter()
                .position(|c| at.col < c.col + c.width)
                .unwrap_or(cells.len());
            if !word(pos) && pos > 0 {
                pos -= 1;
            }
            if !word(pos) {
                return Err(MarkError::Selection);
            }
            let mut first = pos;
            let mut last = pos + 1;
            while first > 0 && word(first - 1) {
                first -= 1;
            }
            while word(last) {
                last += 1;
            }
            cells[first..last].iter().map(|c| c.text.as_str()).collect()
        };
        if value.trim().is_empty()
            || value.len() > crate::marks::MAX_PATTERN_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(MarkError::Selection);
        }
        Ok(value)
    }

    pub fn mark_quick(&mut self, color: u8, remove: bool) -> Result<u64, MarkError> {
        let pattern = self.mark_seed(false)?;
        let options = crate::search::SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let id = self
            .marks
            .rules
            .iter()
            .find(|r| r.row.spec.pattern == pattern && r.row.spec.options == options)
            .map_or(0, |r| r.row.id);
        if remove {
            return if self.mark_remove(id) {
                Ok(id)
            } else {
                Err(MarkError::Missing)
            };
        }
        self.mark_put(
            id,
            MarkSpec {
                pattern,
                options,
                color,
                enabled: true,
            },
        )
    }

    pub fn mark_step(&mut self, id: u64, forward: bool) -> Result<(), MarkError> {
        if self.grid.alt_screen() {
            return Err(MarkError::NoMatch);
        }
        let rule = self
            .marks
            .rules
            .iter()
            .find(|r| r.row.id == id)
            .ok_or(MarkError::Missing)?;
        if !rule.row.spec.enabled {
            return Err(MarkError::NoMatch);
        }
        let matches = self.grid.search_with(&rule.matcher);
        let origin = self.reading.cursor;
        let found = if forward {
            matches
                .iter()
                .find(|m| (m.line, m.start) > (origin.line, origin.col))
                .or(matches.first())
        } else {
            matches
                .iter()
                .rev()
                .find(|m| (m.line, m.start) < (origin.line, origin.col))
                .or(matches.last())
        };
        let Some(found) = found.copied() else {
            return Err(MarkError::NoMatch);
        };
        self.command_unfold_line(found.line);
        self.reading_move(Cell {
            line: found.line,
            col: found.start,
        });
        self.grid.scroll_to_visible(found.line);
        Ok(())
    }

    pub fn search_selection(&mut self, forward: bool) -> Result<(), MarkError> {
        let query = self.mark_seed(true)?;
        let (first, last) = self.selection.ok_or(MarkError::Selection)?.ordered();
        self.search_options.regex = false;
        self.search_options.whole_word = false;
        self.search_query = query;
        self.search_shown = true;
        self.search_recompute();
        self.search_index = if forward {
            self.search_matches
                .iter()
                .position(|m| (m.line, m.start) > (last.line, last.col))
                .or_else(|| (!self.search_matches.is_empty()).then_some(0))
        } else {
            self.search_matches
                .iter()
                .rposition(|m| (m.line, m.end) <= (first.line, first.col))
                .or_else(|| self.search_matches.len().checked_sub(1))
        };
        // A selection-search action finishes the visual motion; revealing its result
        // must not grow the seed selection into a different query for the next action.
        self.reading.visual = None;
        self.reading.operator = None;
        self.search_reveal_active();
        Ok(())
    }
}
