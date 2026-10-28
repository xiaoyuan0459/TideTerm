//! Bounded row-at-a-time motions; never materialize the entire scrollback.
use super::super::TerminalPane;
use super::scan::ReadScan;
use crate::selection::Cell;

const SCAN_LIMIT: usize = 65_536;

impl TerminalPane {
    pub(super) fn read_step(&self, p: Cell, forward: bool) -> Cell {
        let cells = self.grid.reading_cells(p.line);
        let (first, last) = self.grid.reading_rows();
        if forward {
            cells
                .iter()
                .find(|c| c.col > p.col)
                .map(|c| Cell { col: c.col, ..p })
                .unwrap_or_else(|| {
                    if p.line < last {
                        Cell {
                            col: 0,
                            line: self.grid.visible_step(p.line, 1),
                        }
                    } else {
                        p
                    }
                })
        } else {
            cells
                .iter()
                .rev()
                .find(|c| c.col < p.col)
                .map(|c| Cell { col: c.col, ..p })
                .unwrap_or_else(|| {
                    if p.line > first {
                        Cell {
                            line: self.grid.visible_step(p.line, -1),
                            col: self
                                .grid
                                .reading_cells(self.grid.visible_step(p.line, -1))
                                .last()
                                .map_or(0, |c| c.col),
                        }
                    } else {
                        p
                    }
                })
        }
    }

    pub(super) fn read_first(&self, line: i32) -> usize {
        self.grid
            .reading_cells(line)
            .iter()
            .find(|c| !c.text.trim().is_empty())
            .map_or(0, |c| c.col)
    }

    pub(super) fn read_end(&self, line: i32) -> usize {
        self.grid
            .reading_cells(line)
            .iter()
            .rev()
            .find(|c| !c.text.trim().is_empty())
            .map_or(0, |c| c.col)
    }

    fn read_last_line(&self) -> i32 {
        let mut line = self.grid.size().rows as i32 - 1;
        while line > 0
            && self
                .grid
                .reading_cells(line)
                .iter()
                .all(|c| c.text.trim().is_empty())
        {
            line -= 1;
        }
        line.max(self.grid.output_line_number() as i32 - self.grid.total_history() as i32 - 1)
    }

    pub(super) fn read_motion(&self, command: &str, count: usize, counted: bool) -> Option<Cell> {
        let mut p = self.reading.cursor;
        let count = count.clamp(1, 9999);
        let (first, last) = self.grid.reading_rows();
        match command {
            "h" | "l" => {
                let cells = self.grid.reading_cells(p.line);
                let at = cells.iter().rposition(|c| c.col <= p.col).unwrap_or(0);
                let to = if command == "h" {
                    at.saturating_sub(count)
                } else {
                    (at + count).min(cells.len().saturating_sub(1))
                };
                p.col = cells.get(to).map_or(0, |c| c.col);
            }
            "j" | "k" => {
                p.line = self.grid.visible_step(
                    p.line,
                    if command == "j" {
                        count as i32
                    } else {
                        -(count as i32)
                    },
                );
            }
            "0" => p.col = 0,
            "^" => p.col = self.read_first(p.line),
            "$" => {
                p.line = self.grid.visible_step(p.line, count as i32 - 1);
                p.col = self.read_end(p.line);
            }
            "gg" | "G" => {
                p.line = if counted {
                    (first + count as i32 - 1).min(last)
                } else if command == "gg" {
                    first
                } else {
                    self.read_last_line()
                };
                p.col = self.read_first(p.line);
            }
            "H" | "M" | "L" => {
                let rows = self.grid.size().rows;
                let row = match command {
                    "H" => count.saturating_sub(1),
                    "M" => rows / 2,
                    _ => rows.saturating_sub(count),
                }
                .min(rows - 1);
                p.line = self.grid.source_row(row).unwrap_or(last);
                p.col = self.read_first(p.line);
            }
            "w" | "W" | "b" | "B" | "e" | "E" | "ge" | "gE" => {
                let mut budget = SCAN_LIMIT;
                let mut scan = ReadScan::new(self);
                for _ in 0..count {
                    p = Self::read_word(p, command, &mut budget, &mut scan);
                    if budget == 0 {
                        break;
                    }
                }
            }
            "+" | "-" | "{" | "}" => {
                let forward = command == "+" || command == "}";
                let delta = if forward { 1 } else { -1 };
                let mut budget = SCAN_LIMIT / self.grid.size().cols.max(1);
                for _ in 0..count {
                    let blank = |line| {
                        self.grid
                            .reading_cells(line)
                            .iter()
                            .all(|c| c.text.trim().is_empty())
                    };
                    let mut left_nonblank = false;
                    loop {
                        let next = self.grid.visible_step(p.line, delta);
                        if next == p.line || budget == 0 {
                            break;
                        }
                        p.line = next;
                        budget -= 1;
                        if command == "+" || command == "-" {
                            if !blank(p.line) {
                                break;
                            }
                        } else if blank(p.line) {
                            left_nonblank = true;
                        } else if left_nonblank {
                            break;
                        }
                    }
                }
                p.col = self.read_first(p.line);
            }
            "(" | ")" => {
                let mut budget = SCAN_LIMIT;
                let mut scan = ReadScan::new(self);
                for _ in 0..count {
                    p = Self::read_sentence(p, command == ")", &mut budget, &mut scan);
                }
            }
            _ => return None,
        }
        p.col = self.read_col(p.line, p.col);
        Some(p)
    }

    fn read_word(mut p: Cell, command: &str, budget: &mut usize, scan: &mut ReadScan) -> Cell {
        let big = command.ends_with('B') || command.ends_with('E') || command == "W";
        let class = |c: char| {
            if c.is_whitespace() {
                0
            } else if big || c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        let forward = matches!(command, "w" | "W" | "e" | "E");
        let end = matches!(command, "e" | "E" | "ge" | "gE");
        let initial = class(scan.ch(p));
        let step = |p, budget: &mut usize, scan: &mut ReadScan| {
            if *budget == 0 {
                p
            } else {
                *budget -= 1;
                scan.step(p, forward)
            }
        };
        if matches!(command, "w" | "W" | "ge" | "gE") {
            while *budget > 0 {
                let next = step(p, budget, scan);
                if next == p {
                    return p;
                }
                p = next;
                if class(scan.ch(p)) != initial {
                    break;
                }
            }
        } else {
            p = step(p, budget, scan);
        }
        while class(scan.ch(p)) == 0 && *budget > 0 {
            let next = step(p, budget, scan);
            if next == p {
                return p;
            }
            p = next;
        }
        if forward && !end || !forward && end {
            return p;
        }
        let current = class(scan.ch(p));
        while *budget > 0 {
            let next = step(p, budget, scan);
            if next == p || class(scan.ch(next)) != current {
                break;
            }
            p = next;
        }
        p
    }

    fn read_sentence(mut p: Cell, forward: bool, budget: &mut usize, scan: &mut ReadScan) -> Cell {
        let origin = p;
        while *budget > 0 {
            *budget -= 1;
            let next = scan.step(p, forward);
            if next == p {
                return p;
            }
            p = next;
            if let Some(start) = scan.sentence_start(p)
                && (forward || (start.line, start.col) < (origin.line, origin.col))
            {
                return start;
            }
            if ".!?。！？".contains(scan.ch(p)) {
                let punctuation = p;
                let after = scan.step(p, true);
                if scan.ch(after).is_whitespace() {
                    p = after;
                    while *budget > 0 && scan.ch(p).is_whitespace() {
                        *budget -= 1;
                        let next = scan.step(p, true);
                        if next == p {
                            break;
                        }
                        p = next;
                    }
                    if forward || (p.line, p.col) < (origin.line, origin.col) {
                        return p;
                    }
                    p = punctuation;
                }
            }
        }
        p
    }
}
