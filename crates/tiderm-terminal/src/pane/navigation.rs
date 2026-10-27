//! Two bounded source anchors for a line-navigation draft; typing never moves the viewport.
use super::TerminalPane;
use crate::selection::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LineError {
    Unavailable = 1,
    Query,
    Range,
    Expired,
}
pub struct LineContext {
    pub id: u64,
    pub current: u64,
    pub total: u64,
}
pub struct LinePreview {
    pub token: u64,
    pub line: u64,
    pub text: String,
}
struct Draft {
    id: u64,
    origin: Cell,
    target: Option<(u64, Cell)>,
}
#[derive(Default)]
pub(super) struct Navigation {
    serial: u64,
    draft: Option<Draft>,
}
impl Navigation {
    fn next(&mut self) -> Result<u64, LineError> {
        self.serial = self.serial.checked_add(1).ok_or(LineError::Expired)?;
        Ok(self.serial)
    }
    pub(super) fn invalidate(&mut self) {
        self.draft = None;
    }
    pub(super) fn remap(&mut self, map: &impl Fn(Cell) -> Option<Cell>) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(origin) = map(draft.origin) else {
            self.invalidate();
            return;
        };
        draft.origin = origin;
        draft.target = draft
            .target
            .and_then(|(token, cell)| map(cell).map(|cell| (token, cell)));
    }
}

impl TerminalPane {
    pub fn line_begin(&mut self) -> Result<LineContext, LineError> {
        if self.grid.alt_screen() {
            return Err(LineError::Unavailable);
        }
        let origin = if self.reading.active {
            self.reading.cursor
        } else {
            Cell {
                line: (self.grid.output_line_number() as i64 - self.grid.total_history() as i64 - 1)
                    as i32,
                col: self.grid.cursor_col(),
            }
        };
        let id = self.navigation.next()?;
        self.navigation.draft = Some(Draft {
            id,
            origin,
            target: None,
        });
        Ok(LineContext {
            id,
            current: self.line_number(origin.line),
            total: self.grid.buffer_lines(),
        })
    }
    fn line_number(&self, line: i32) -> u64 {
        (self.grid.total_history() as i64 + i64::from(line) + 1).max(1) as u64
    }
    pub fn line_end(&mut self, context: u64) {
        if self
            .navigation
            .draft
            .as_ref()
            .is_some_and(|d| d.id == context)
        {
            self.navigation.invalidate();
        }
    }
    pub fn line_preview(&mut self, context: u64, query: &str) -> Result<LinePreview, LineError> {
        if self.grid.alt_screen() {
            return Err(LineError::Unavailable);
        }
        let draft = self
            .navigation
            .draft
            .as_mut()
            .filter(|d| d.id == context)
            .ok_or(LineError::Expired)?;
        draft.target = None;
        let origin = draft.origin;
        let raw = query.trim();
        let relative = raw.starts_with(['+', '-']);
        let digits = if relative { &raw[1..] } else { raw };
        if raw.len() > 32 || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(LineError::Query);
        }
        let n = digits.parse::<u64>().map_err(|_| LineError::Range)?;
        let base = self.line_number(origin.line);
        let line = match raw.as_bytes().first() {
            Some(b'+') => base.checked_add(n),
            Some(b'-') => base.checked_sub(n),
            _ => Some(n),
        }
        .filter(|&line| line >= 1 && line <= self.grid.buffer_lines())
        .ok_or(LineError::Range)?;
        let target = Cell {
            line: i32::try_from(line as i64 - self.grid.total_history() as i64 - 1)
                .map_err(|_| LineError::Range)?,
            col: 0,
        };
        let token = self.navigation.next()?;
        if let Some(draft) = &mut self.navigation.draft {
            draft.target = Some((token, target));
        }
        let mut text = String::new();
        for cell in self.grid.reading_cells(target.line) {
            if text.len() + cell.text.len() > 1024 {
                break;
            }
            text.push_str(&cell.text);
        }
        Ok(LinePreview {
            token,
            line,
            text: text.trim_end().to_owned(),
        })
    }
    pub fn line_can_commit(&self, context: u64, token: u64) -> bool {
        !self.grid.alt_screen()
            && self
                .navigation
                .draft
                .as_ref()
                .is_some_and(|d| d.id == context && d.target.is_some_and(|(id, _)| id == token))
    }
    /// The host enters reading mode after admission and before consuming the target.
    pub fn line_commit(&mut self, context: u64, token: u64) -> bool {
        if !self.line_can_commit(context, token) {
            return false;
        }
        let Some(target) = self
            .navigation
            .draft
            .as_mut()
            .and_then(|d| d.target.take())
            .map(|(_, cell)| cell)
        else {
            return false;
        };
        self.command_unfold_line(target.line);
        self.reading_cancel();
        self.selection_clear();
        self.reading_move(target);
        self.grid.scroll_to_visible(target.line);
        true
    }
}
