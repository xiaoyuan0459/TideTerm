//! Exact Shell input spans supplied by OSC 133 B; independent of prompt decoration.
use super::{Commands, TerminalPane};

impl TerminalPane {
    /// Once explicit input markers were observed, callers must not guess a prompt while a program runs.
    /// A repaint that erased the live anchor without replaying B is the one exception:
    /// the shell is still at its input prompt and only the conventional prompt remains.
    pub fn has_command_input_markers(&self) -> bool {
        self.commands.marked && !self.commands.repainted
    }

    /// A current B input lease survives source-coordinate remapping, but is
    /// retired on submit/output/clear. Hosts may recognize a known prompt redraw
    /// within this lease when ConPTY has repainted the visible prompt.
    pub fn command_input_active(&self) -> bool {
        self.commands.input.is_some() && !self.grid.alt_screen()
    }

    /// Editable input through an end-of-line caret. Preserve user spaces and graphemes.
    pub fn command_input_prefix(&self) -> Option<String> {
        if !self.grid.cursor_at_line_end() {
            return None;
        }
        self.marked_input(true)
    }

    /// Full logical input when Enter is pressed after editing in the middle of a line.
    pub fn command_input_line(&self) -> Option<String> {
        self.marked_input(false)
    }

    /// Bounded physical rows at a live input caret, before logical-wrap joining.
    pub fn live_input_physical_rows(&self) -> Option<(i32, Vec<String>)> {
        if !self.has_command_input_markers()
            || !self.command_input_active()
            || self.grid.display_offset() != 0
            || self.grid.cursor_row().is_none()
            || !self.grid.cursor_at_line_end()
        {
            return None;
        }
        let cursor = Commands::cursor(&self.grid);
        let first = cursor.line.saturating_sub(15).max(0);
        let mut rows = Vec::new();
        let mut bytes = 0;
        for row in first..=cursor.line {
            let end = if row == cursor.line {
                cursor.col
            } else {
                self.grid.size().cols
            };
            let mut text = String::new();
            for cell in self
                .grid
                .reading_cells(row)
                .into_iter()
                .filter(|cell| cell.col < end)
            {
                if cell.col + cell.width > end || bytes + cell.text.len() > 4096 {
                    return None;
                }
                bytes += cell.text.len();
                text.push_str(&cell.text);
            }
            rows.push(text);
        }
        Some((first, rows))
    }

    /// Re-anchor an existing input lease after its host recognizes a prompt redraw.
    pub fn restore_live_prompt_at_cursor(&mut self, prompt_line: i32) -> bool {
        if !self.has_command_input_markers()
            || !self.command_input_active()
            || self.grid.display_offset() != 0
            || self.grid.cursor_row().is_none()
            || !self.grid.cursor_at_line_end()
        {
            return false;
        }
        let cursor = Commands::cursor(&self.grid);
        if prompt_line < 0 || prompt_line > cursor.line {
            return false;
        }
        let mut prompt = cursor;
        prompt.line = prompt_line;
        prompt.col = 0;
        self.commands.prompt = Some(prompt);
        self.commands.input = Some(cursor);
        true
    }

    fn marked_input(&self, prefix: bool) -> Option<String> {
        if self.grid.alt_screen() || self.grid.display_offset() != 0 {
            return None;
        }
        let start = self.commands.input?;
        let cursor = Commands::cursor(&self.grid);
        let mut last = cursor.line;
        if !prefix {
            while self.grid.line_wraps(last) {
                last += 1;
                if last - start.line > 512 {
                    return None;
                }
            }
        }
        if start.line < self.grid.reading_rows().0 || last < start.line || last - start.line > 512 {
            return None;
        }
        let mut text = String::new();
        for row in start.line..=last {
            let from = if row == start.line { start.col } else { 0 };
            let end = if prefix && row == last {
                cursor.col
            } else {
                self.grid.size().cols
            };
            if end < from {
                return None;
            }
            for cell in self
                .grid
                .reading_cells(row)
                .into_iter()
                .filter(|c| c.col >= from && c.col < end)
            {
                if cell.col + cell.width > end {
                    return None;
                }
                text.push_str(&cell.text);
                if text.len() > 4096 {
                    return None;
                }
            }
            if row < last && !self.grid.line_wraps(row) {
                text.push('\n');
            }
        }
        if !prefix {
            text.truncate(text.trim_end().len());
        }
        Some(text)
    }
}
