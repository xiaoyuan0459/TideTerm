//! Small command grammar with counts and explicitly bounded copy registers.
use super::{ReadAction, TerminalPane};
use crate::selection::Cell;

const REGISTER_BUDGET: usize = 1024 * 1024;

impl TerminalPane {
    pub(super) fn reading_command(&mut self, ch: char) -> ReadAction {
        // Text operations reveal a marker's source row before inspecting its cells.
        if self.reading.prefix.is_empty()
            && "hl0^$wWbBeEfFtT;,vVyo".contains(ch)
            && self.grid.fold_marker(self.reading.cursor.line).is_some()
        {
            self.command_unfold_line(self.reading.cursor.line);
        }
        if self.reading.prefix == "\"" {
            self.reading.prefix.clear();
            self.reading.register = if ch.is_ascii_alphanumeric() || ch == '+' {
                ch.to_ascii_lowercase()
            } else {
                '"'
            };
            return ReadAction::None;
        }
        if self.reading.prefix.len() == 1 && "fFtT".contains(self.reading.prefix.as_str()) {
            let kind = self.reading.prefix.chars().next().unwrap();
            self.reading.prefix.clear();
            self.reading.last_find = Some((kind, ch));
            let count = self.take_read_count();
            if let Some(target) = self.read_find(kind, ch, count) {
                self.read_finish(target, "f");
            } else {
                self.reading_cancel_operator();
            }
            return ReadAction::None;
        }
        if ch.is_ascii_digit() && (ch != '0' || self.reading.count != 0) {
            self.reading.count =
                (self.reading.count * 10 + ch.to_digit(10).unwrap() as usize).min(9999);
            return ReadAction::None;
        }
        if ch == '"' {
            self.reading.prefix = "\"".into();
            return ReadAction::None;
        }
        if self.reading.prefix.is_empty() {
            match ch {
                'i' => {
                    self.reading_cancel();
                    return ReadAction::Exit;
                }
                'p' | 'P' => {
                    let register = self.reading.register;
                    let text = if register == '+' {
                        self.clipboard.paste()
                    } else {
                        self.reading.registers.get(&register).cloned()
                    };
                    self.reading_cancel();
                    if let Some(text) = text.filter(|s| !s.is_empty()) {
                        return ReadAction::Paste {
                            bytes: super::super::prepare_paste(&text, self.grid.bracketed_paste())
                                .into_bytes(),
                            stay: ch == 'P',
                        };
                    }
                    return ReadAction::None;
                }
                'v' | 'V' => {
                    self.reading.visual_column = false;
                    if self.reading.visual.is_some() {
                        self.reading.visual = None;
                    } else {
                        self.reading_begin_visual(false);
                    }
                    return ReadAction::None;
                }
                // Swap the two ends of the live selection: vim's `o`, and the keyboard half of
                // "swap anchor". Copy text is unchanged — only the direction the next motion grows.
                'o' if self.reading.visual.is_some() => {
                    self.reading_swap_anchor();
                    return ReadAction::None;
                }
                'y' if self.reading.visual.is_some() => {
                    self.read_copy(false);
                    return ReadAction::None;
                }
                'y' if self.reading.operator.is_none() => {
                    let count = self.take_read_count();
                    self.reading.operator = Some((self.reading.cursor, count));
                    return ReadAction::None;
                }
                'y' => {
                    let (start, count) = self.reading.operator.take().unwrap();
                    let lines = count.saturating_mul(self.take_read_count()).min(9999) as i32;
                    let end_line = (start.line + lines - 1).min(self.grid.reading_rows().1);
                    self.reading_select(
                        Cell { col: 0, ..start },
                        Cell {
                            line: end_line,
                            col: self.read_end(end_line),
                        },
                    );
                    self.read_copy(true);
                    return ReadAction::None;
                }
                'g' | 'z' | 'f' | 'F' | 't' | 'T' => {
                    self.reading.prefix.push(ch);
                    return ReadAction::None;
                }
                ';' | ',' => {
                    if let Some((kind, target)) = self.reading.last_find {
                        let kind = if ch == ',' {
                            match kind {
                                'f' => 'F',
                                'F' => 'f',
                                't' => 'T',
                                _ => 't',
                            }
                        } else {
                            kind
                        };
                        let count = self.take_read_count();
                        if let Some(target) = self.read_find(kind, target, count) {
                            self.read_finish(target, "f");
                        } else {
                            self.reading_cancel_operator();
                        }
                    }
                    return ReadAction::None;
                }
                'n' | 'N' => {
                    for _ in 0..self.take_read_count() {
                        self.search_step(ch == 'n');
                    }
                    self.reading_cancel_operator();
                    return ReadAction::None;
                }
                _ => {}
            }
        }
        let counted = self.reading.count != 0;
        let count = self.take_read_count();
        let command = format!("{}{ch}", std::mem::take(&mut self.reading.prefix));
        if matches!(command.as_str(), "za" | "zo" | "zO" | "zc" | "zC") {
            self.reading_cancel_operator();
            self.command_fold_current_recursive(
                match command.as_str() {
                    "za" => None,
                    "zo" | "zO" => Some(false),
                    _ => Some(true),
                },
                matches!(command.as_str(), "zO" | "zC"),
            );
            return ReadAction::None;
        }
        if matches!(command.as_str(), "zm" | "zM" | "zr" | "zR") {
            self.reading_cancel_operator();
            self.command_fold_all(matches!(command.as_str(), "zm" | "zM"));
            return ReadAction::None;
        }
        if matches!(command.as_str(), "zj" | "zk") {
            self.reading_cancel_operator();
            for _ in 0..count.min(500) {
                if !self.command_step(command == "zj") {
                    break;
                }
            }
            return ReadAction::None;
        }
        if matches!(command.as_str(), "zt" | "zz" | "zb") {
            let row = match command.as_str() {
                "zt" => 0,
                "zz" => self.grid.size().rows / 2,
                _ => self.grid.size().rows - 1,
            };
            let top = (self.grid.visible_index(self.reading.cursor.line) - row as i32)
                .clamp(0, self.grid.visible_index(0));
            self.grid
                .set_view_offset((-self.grid.source_index(top)).max(0) as usize);
            self.reading_clamp(true);
            return ReadAction::None;
        }
        let count = count
            .saturating_mul(self.reading.operator.map_or(1, |(_, c)| c))
            .min(9999);
        if let Some(target) = self.read_motion(&command, count, counted) {
            self.read_finish(target, &command);
        } else {
            self.reading_cancel_operator();
        }
        ReadAction::None
    }

    fn take_read_count(&mut self) -> usize {
        std::mem::take(&mut self.reading.count).max(1)
    }

    fn reading_cancel_operator(&mut self) {
        self.reading.operator = None;
        self.reading.register = '"';
    }

    fn read_finish(&mut self, target: Cell, command: &str) {
        if let Some((start, _)) = self.reading.operator.take() {
            let (a, b) = if (target.line, target.col) >= (start.line, start.col) {
                let end = if matches!(command, "w" | "W" | "j" | "k") {
                    self.read_step(target, false)
                } else {
                    target
                };
                (start, end)
            } else {
                (target, self.read_step(start, false))
            };
            self.reading_select(a, b);
            self.read_copy(false);
            self.reading.cursor = start;
        } else {
            self.reading_move(target);
        }
    }

    fn read_copy(&mut self, newline: bool) {
        let value = self.selection_text_result();
        if value.is_err() {
            self.set_toast("Selection exceeds the 1 MiB copy budget");
        }
        if let Ok(Some(mut text)) = value {
            if newline {
                text.push('\n');
            }
            let register = self.reading.register;
            let old = self
                .reading
                .registers
                .iter()
                .filter(|(k, _)| **k != register && **k != '"')
                .map(|(_, v)| v.len())
                .sum::<usize>();
            let total = old
                + text.len()
                    * if register == '"' || register == '+' {
                        1
                    } else {
                        2
                    };
            if total <= REGISTER_BUDGET {
                if register == '+' {
                    self.clipboard.copy(&text);
                } else if register != '"' {
                    self.reading.registers.insert(register, text.clone());
                }
                self.reading.registers.insert('"', text);
            } else {
                self.set_toast("Copy exceeds the 1 MiB reading register budget");
            }
        }
        self.reading_cancel();
    }

    fn read_find(&self, kind: char, target: char, count: usize) -> Option<Cell> {
        let p = self.reading.cursor;
        let cells = self.grid.reading_cells(p.line);
        let forward = kind.is_ascii_lowercase();
        let mut matches: Vec<_> = cells
            .iter()
            .filter(|c| {
                if forward {
                    c.col > p.col
                } else {
                    c.col < p.col
                }
            })
            .filter(|c| c.text.starts_with(target))
            .collect();
        if !forward {
            matches.reverse();
        }
        let found = matches.get(count - 1)?;
        let mut point = Cell {
            col: found.col,
            ..p
        };
        if kind == 't' || kind == 'T' {
            point = self.read_step(point, !forward);
        }
        Some(point)
    }
}
