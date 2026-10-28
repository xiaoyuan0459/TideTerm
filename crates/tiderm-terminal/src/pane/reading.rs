//! Local command state is per pane; it never writes to a transport.
mod commands;
mod motion;
mod scan;
use super::TerminalPane;
use crate::selection::{Cell, Selection};
use std::collections::HashMap;

pub enum ReadAction {
    None,
    Exit,
    Paste { bytes: Vec<u8>, stay: bool },
}

pub(super) struct Reading {
    pub active: bool,
    pub cursor: Cell,
    pub visual: Option<Cell>,
    pub visual_column: bool,
    pub count: usize,
    pub prefix: String,
    pub register: char,
    pub operator: Option<(Cell, usize)>,
    pub last_find: Option<(char, char)>,
    pub registers: HashMap<char, String>,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            active: false,
            cursor: Cell { col: 0, line: 0 },
            visual: None,
            visual_column: false,
            count: 0,
            prefix: String::new(),
            register: '"',
            operator: None,
            last_find: None,
            registers: HashMap::new(),
        }
    }
}

impl TerminalPane {
    pub fn reading_search_ready(&self) -> bool {
        self.reading.active && self.reading.prefix.is_empty() && self.reading.operator.is_none()
    }
    pub fn set_reading_mode(&mut self, active: bool) {
        self.reading.active = active;
        self.grid.set_follow_output(!active);
        self.reading_cancel();
        if active {
            self.reading_reset_cursor();
        } else {
            self.scroll_to_bottom();
        }
    }

    pub(super) fn reading_reset_cursor(&mut self) {
        let row = self.grid.cursor_row().unwrap_or(0) as i32;
        self.reading.cursor = Cell {
            col: self.grid.cursor_col(),
            line: self.grid.source_row(row as usize).unwrap_or(0),
        };
        self.reading_clamp(false);
    }

    pub(super) fn reading_cancel(&mut self) {
        self.reading.count = 0;
        self.reading.prefix.clear();
        self.reading.operator = None;
        self.reading.register = '"';
        self.reading.visual = None;
        self.reading.visual_column = false;
    }

    pub(super) fn reading_clamp(&mut self, visible: bool) {
        let (first, last) = self.grid.reading_rows();
        let top = self.grid.source_row(0).unwrap_or(first);
        let bottom = self
            .grid
            .source_row(self.grid.size().rows - 1)
            .unwrap_or(last);
        let range = if visible {
            (first.max(top), last.min(bottom))
        } else {
            (first, last)
        };
        self.reading.cursor.line = self
            .grid
            .visible_step(self.reading.cursor.line.clamp(range.0, range.1), 0);
        self.reading.cursor.col = self.read_col(self.reading.cursor.line, self.reading.cursor.col);
    }

    fn read_col(&self, line: i32, col: usize) -> usize {
        self.grid
            .reading_cells(line)
            .iter()
            .rev()
            .find(|c| c.col <= col)
            .map_or(0, |c| c.col)
    }

    pub(super) fn reading_caret_cell(&self) -> Option<(usize, usize)> {
        if !self.reading.active {
            return None;
        }
        self.grid
            .screen_row(self.reading.cursor.line)
            .map(|row| (self.reading.cursor.col, row))
    }

    pub(super) fn reading_snapshot(&self, snap: &mut crate::grid::GridSnapshot) {
        if self.reading.active {
            snap.cursor_visible = false;
            if let Some(cursor) = self.reading_caret_cell() {
                snap.cursor = cursor;
                snap.cursor_visible = true;
            }
        }
    }

    pub(super) fn reading_select(&mut self, start: Cell, mut end: Cell) {
        if self.selection_locked {
            return;
        }
        end = self.widen_head(start, end);
        let kind = if self.reading.visual_column && self.reading.visual.is_some() {
            crate::selection::SelectionKind::Column
        } else {
            crate::selection::SelectionKind::Stream
        };
        let mut sel = Selection::with_kind(start, kind);
        sel.update(end);
        sel.dragged = true; // A keyboard selection can contain exactly one glyph.
        if !self.has_selection() {
            self.next_selection_id();
        }
        self.selection = Some(sel);
    }

    pub(super) fn reading_move(&mut self, target: Cell) {
        if self.grid.fold_hidden(target.line) {
            self.command_unfold_line(target.line);
        }
        self.reading.cursor = target;
        self.reading_clamp(false);
        self.grid.scroll_to_visible(self.reading.cursor.line);
        if let Some(anchor) = self.reading.visual {
            self.reading_select(anchor, self.reading.cursor);
        }
    }

    pub fn reading_scroll(&mut self, lines: i32) {
        if !self.grid.alt_screen() {
            self.scroll_by(lines);
        }
        self.reading_clamp(true);
        if let Some(anchor) = self.reading.visual {
            self.reading_select(anchor, self.reading.cursor);
        }
    }

    pub fn reading_key(&mut self, key: u32, mods: u32, text: &str) -> ReadAction {
        if !self.reading.active {
            return ReadAction::None;
        }
        if key == 15 {
            self.reading_cancel();
            self.selection_clear();
            return ReadAction::None;
        }
        if matches!(key, 12 | 13) && mods & 6 == 0 && self.selection_count() > 1 {
            self.selection_cycle(key == 12 && mods & 1 == 0);
            return ReadAction::None;
        }
        if mods & 7 == 4 && matches!(text, "v" | "V" | "\u{16}") {
            if self.reading.visual.is_some() && self.reading.visual_column {
                self.reading.visual = None;
            } else {
                self.reading_begin_visual(true);
            }
            return ReadAction::None;
        }
        if mods & 7 == 4 {
            let rows = self.grid.size().rows as i32;
            let count = self.reading.count.clamp(1, 9999) as i32;
            let scroll = match text.chars().next().map(|c| c.to_ascii_lowercase()) {
                Some('e' | '\u{5}') => Some(-count),
                Some('y' | '\u{19}') => Some(count),
                Some('d' | '\u{4}') => Some(-count * (rows / 2).max(1)),
                Some('u' | '\u{15}') => Some(count * (rows / 2).max(1)),
                Some('f' | '\u{6}') => Some(-count * rows),
                Some('b' | '\u{2}') => Some(count * rows),
                _ => None,
            };
            if let Some(lines) = scroll {
                self.reading_scroll(lines);
            }
            self.reading.count = 0;
            return ReadAction::None;
        }
        if mods & 6 != 0 {
            return ReadAction::None;
        }
        let mapped = match key {
            1 => Some('k'),
            2 => Some('j'),
            3 | 11 => Some('h'),
            4 => Some('l'),
            5 => Some('0'),
            6 => Some('$'),
            _ => None,
        };
        if key == 7 || key == 8 {
            let rows = self.grid.size().rows as i32;
            self.reading_scroll(if key == 7 { rows } else { -rows });
            return ReadAction::None;
        }
        let Some(ch) = mapped.or_else(|| text.chars().next()) else {
            return ReadAction::None;
        };
        if mapped.is_none() && text.chars().count() != 1 {
            return ReadAction::None;
        }
        self.reading_command(ch)
    }
}
