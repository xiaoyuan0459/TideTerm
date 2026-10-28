//! Read-only terminal metadata, independent of viewport rendering.
use super::TerminalPane;

impl TerminalPane {
    /// Whether the active buffer belongs to a full-screen application.
    pub fn alternate_screen(&self) -> bool {
        self.grid.alt_screen()
    }
    /// Whether aggregate clipboard input must carry the application's paste envelope.
    pub fn bracketed_paste(&self) -> bool {
        self.grid.bracketed_paste()
    }

    /// Live application keyboard modes, including changes requested by the remote program.
    pub fn app_cursor(&self) -> bool {
        self.grid.app_cursor()
    }
    pub fn app_keypad(&self) -> bool {
        self.grid.app_keypad()
    }

    /// Gutter strip width in TEXT cells for the current buffer (0 = gutter off). The
    /// host subtracts this from the pane body columns when deriving the grid size.
    pub fn gutter_cells(&self, on: bool) -> usize {
        if on {
            crate::render::gutter_cells(self.grid.buffer_lines())
        } else {
            0
        }
    }
    /// 压缩回滚层的估算字节（内存探针用；不含 alacritty 活窗口与帧缓冲）。
    pub fn scrollback_bytes(&self) -> usize {
        self.grid.scrollback_bytes()
    }
    /// 本窗格回滚层的字节预算（`with_history` 的 cap）。与 `scrollback_bytes` 成对上报，
    /// 内存探针才能看出"预算有没有真的落到窗格上"。
    pub fn scrollback_budget(&self) -> usize {
        self.grid.scrollback_budget()
    }
    /// 保留历史总行数（活窗口 + 压缩层）。
    pub fn total_history(&self) -> usize {
        self.grid.total_history()
    }
    /// Current grid size in `(cols, rows)`.
    pub fn grid_size(&self) -> (usize, usize) {
        let s = self.grid.size();
        (s.cols, s.rows)
    }
    /// Logical prompt line at submission. Alternate screens and scrollback are excluded;
    /// this is also valid when Enter is pressed after mid-line editing.
    pub fn command_line(&self) -> Option<String> {
        if self.grid.alt_screen() || self.grid.display_offset() != 0 {
            return None;
        }
        let mut row = self.grid.cursor_row()? as i32;
        let mut scanned = 0;
        while self.grid.line_text(row - 1).is_some() && self.grid.line_wraps(row - 1) {
            row -= 1;
            scanned += 1;
            if scanned > 512 {
                return None;
            }
        }
        let mut line = String::new();
        loop {
            line.push_str(&self.grid.line_text(row)?);
            if line.len() > 4096 {
                return None;
            }
            if !self.grid.line_wraps(row) {
                return Some(line);
            }
            row += 1;
        }
    }
    /// Logical prompt input through an end-of-line caret, preserving typed spaces
    /// and graphemes across soft wrapping. Padding and wide spacers are excluded.
    pub fn command_prefix(&self) -> Option<String> {
        if self.grid.alt_screen()
            || self.grid.display_offset() != 0
            || !self.grid.cursor_at_line_end()
        {
            return None;
        }
        let last = self.grid.cursor_row()? as i32;
        let mut first = last;
        while self.grid.line_text(first - 1).is_some() && self.grid.line_wraps(first - 1) {
            first -= 1;
            if last - first > 512 {
                return None;
            }
        }
        let mut text = String::new();
        for row in first..=last {
            let end = if row == last {
                self.grid.cursor_boundary_col()
            } else {
                self.grid.size().cols
            };
            for cell in self
                .grid
                .reading_cells(row)
                .into_iter()
                .take_while(|cell| cell.col < end)
            {
                if cell.col + cell.width > end {
                    return None;
                }
                text.push_str(&cell.text);
                if text.len() > 4096 {
                    return None;
                }
            }
        }
        Some(text)
    }
    /// Buffer line receiving output, even when the user is reading scrollback.
    pub fn output_line_number(&self) -> u64 {
        self.grid.output_line_number()
    }
    /// The visible screen as plain text — one line per viewport row (blank cells as spaces),
    /// with trailing whitespace and trailing blank lines trimmed. Lets a host feed an ambient
    /// summariser the *rendered* screen (what the user actually sees) instead of the raw redraw
    /// byte stream, so a continuously-repainting TUI (e.g. an agent CLI) is captured cleanly
    /// rather than as redraw noise.
    pub fn screen_text(&self) -> String {
        let snap = self.grid.snapshot();
        let mut lines: Vec<String> = (0..snap.rows)
            .map(|r| Self::row_text(&snap, r).trim_end().to_string())
            .collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}
