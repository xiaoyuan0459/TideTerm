//! TermGrid: navigation.
use super::*;

impl TermGrid {
    /// Whether the application has enabled **bracketed paste** mode (DECSET 2004). When on, a
    /// paste must be wrapped in `ESC[200~ … ESC[201~` so the shell/line-editor (e.g. PSReadLine)
    /// treats it as one literal insertion instead of replaying each embedded newline as Enter —
    /// which is what fragments a multi-line paste across `>>` continuation prompts and strands the
    /// caret. See [`TerminalPane::paste_from_clipboard`](crate::pane::TerminalPane).
    pub fn bracketed_paste(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
    }

    // ---- Scrollback access + scrolling (for in-pane search) ---------------------------------

    /// How far the viewport is scrolled up into history, in lines (0 = pinned to the bottom /
    /// live prompt) — the virtual offset into the compressed store + alacritty window.
    /// Viewport row `r` shows absolute grid line `r - display_offset`.
    pub fn display_offset(&self) -> usize {
        self.viewport.offset()
    }

    /// The cursor's column (0-based grid cell). Pairs with [`cursor_row`](Self::cursor_row) to
    /// compute the type-over-selection edit sequence (how far the selection sits from the caret).
    /// The text of the cursor's own row (raw window coordinates), cell padding mapped to
    /// spaces. CommandSuggest (G15) uses it as the typed input prefix. Cheap — one row read.
    pub fn cursor_line_text(&self) -> String {
        let line = self.term.grid().cursor.point.line.0;
        self.line_text_at(line).unwrap_or_default()
    }

    /// True when the cursor sits at (or past) the end of the typed text on its row — i.e.
    /// the user is appending at the prompt, not editing mid-line or inside a TUI.
    pub fn cursor_at_line_end(&self) -> bool {
        let last = self.cursor_line_text().trim_end().chars().count();
        self.cursor_boundary_col() >= last
    }

    pub fn cursor_col(&self) -> usize {
        self.term.grid().cursor.point.column.0
    }

    /// Whether viewport row `row` CONTINUES onto the next row — alacritty's `WRAPLINE` flag on
    /// its last cell — i.e. rows `row` and `row+1` render one wrapped logical line. Lets
    /// type-over-selection treat a long shell input that soft-wrapped across visual rows as the
    /// single editable line it is (the line editor's arrows/backspace walk straight through the
    /// wrap, so linear cell distances stay valid).
    pub fn row_wraps(&self, row: usize) -> bool {
        if row >= self.size.rows || self.size.cols == 0 {
            return false;
        }
        self.source_row(row)
            .is_some_and(|line| self.fold_marker(line).is_none() && self.wrap_at(line))
    }

    /// Whether the application has switched to the **alternate screen** (vim, htop, full-screen
    /// TUIs). Type-over-selection must not fire there: its arrow/backspace bytes would be app
    /// commands (vim motions), not line edits.
    pub fn alt_screen(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    }

    /// Whether the application has enabled **mouse reporting** (DECSET 1000/1002/1003 — any of
    /// click / drag / motion tracking). When on, a wheel notch should be forwarded to the pty as a
    /// mouse event so the app (Claude Code, vim, less, htop) scrolls its own view, rather than the
    /// terminal moving a scrollback viewport the app doesn't have. See [`crate::pane`]'s wheel path.
    pub fn mouse_mode(&self) -> bool {
        self.term
            .mode()
            .intersects(alacritty_terminal::term::TermMode::MOUSE_MODE)
    }

    /// Whether the application requested **SGR** (1006) mouse encoding. Picks the wheel-report
    /// wire format: SGR (`ESC[<Cb;Cx;Cy M`) when on, else legacy X10 (`ESC[M` + 3 bytes).
    pub fn sgr_mouse(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::SGR_MOUSE)
    }

    /// Whether **application cursor keys** (DECCKM) are active. Selects the arrow-key encoding used
    /// for alternate-scroll (alt-screen, no mouse mode): `ESC O A/B` when on, else `ESC [ A/B`.
    pub fn app_keypad(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::APP_KEYPAD)
    }

    pub fn app_cursor(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::APP_CURSOR)
    }

    /// Whether the app requested **button-event (drag) tracking** (DECSET 1002): report pointer
    /// motion only while a button is held. Gates motion-event forwarding for mouse-grab apps.
    pub fn mouse_drag(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::MOUSE_DRAG)
    }

    /// Whether the app requested **any-event motion tracking** (DECSET 1003): report all pointer
    /// motion, button held or not.
    pub fn mouse_any_motion(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::MOUSE_MOTION)
    }

    /// Scrollback geometry for the scrollbar/HUD overlays: `(history_lines, viewport_rows,
    /// display_offset)`. `history_lines` is the **total** scrollback (compressed store +
    /// alacritty window); `display_offset` is the virtual offset into it (0 = pinned to the
    /// live edge). Total buffer height = `history_lines + viewport_rows`. In the alternate
    /// screen history is 0, so the overlays naturally vanish.
    pub fn scroll_metrics(&self) -> (usize, usize, usize) {
        (
            self.visible_index(0).max(0) as usize,
            self.size.rows,
            (self.visible_index(0) - self.visible_index(-(self.viewport.offset() as i32))).max(0)
                as usize,
        )
    }

    /// Text of absolute grid `line` (negative = scrollback, `0..rows` = live viewport), one
    /// char per cell with blanks as spaces and no right-trim, or `None` when the line is
    /// outside the retained buffer. Serves both the live window and the compressed store.
    pub(super) fn line_text_at(&self, abs: i32) -> Option<String> {
        let cols = self.size.cols;
        let h = self.term.grid().history_size() as i32;
        let s = self.stored_rows() as i32;
        if abs < -(s + h) || abs > (self.size.rows as i32 - 1) {
            return None;
        }
        if abs >= -h {
            let row = &self.term.grid()[Line(abs)];
            let mut out = String::with_capacity(cols);
            for c in 0..cols {
                let ch = row[Column(c)].c;
                out.push(if ch == '\0' { ' ' } else { ch });
            }
            Some(out)
        } else {
            let idx = (s + h + abs) as usize;
            self.store.row(idx).map(|ld| {
                // Stored rows drop trailing blank columns; pad back to the row's real width so
                // this stays "one char per cell, no right-trim" like the live path above.
                let cols = ld.cols();
                let mut out = String::with_capacity(cols);
                for c in 0..cols {
                    let ch = ld.chars.get(c).copied().unwrap_or(' ' as u16);
                    let ch = char::from_u32(ch as u32).unwrap_or(' ');
                    out.push(if ch == '\0' { ' ' } else { ch });
                }
                out
            })
        }
    }

    /// Whether absolute grid `line` CONTINUES onto the next line — `WRAPLINE` on its last cell
    /// (live window) or the stored wrap flag (compressed store).
    pub(super) fn wrap_at(&self, abs: i32) -> bool {
        let cols = self.size.cols;
        if cols == 0 {
            return false;
        }
        let h = self.term.grid().history_size() as i32;
        let s = self.stored_rows() as i32;
        if abs >= -h {
            let last = Column(cols - 1);
            self.term.grid()[Line(abs)][last]
                .flags
                .contains(Flags::WRAPLINE)
        } else if abs >= -(s + h) {
            let idx = (s + h + abs) as usize;
            self.store.row(idx).map(|ld| ld.wrap).unwrap_or(false)
        } else {
            false
        }
    }

    /// The text of absolute grid `line` (negative = scrollback, `0..rows` = live viewport), one
    /// char per cell with blanks as spaces and no right-trim, or `None` when the line is outside
    /// the retained buffer. Lets selection text-extraction reach scrollback rows the viewport
    /// snapshot can't (a selection anchored in history while scrolled).
    pub fn line_text(&self, line: i32) -> Option<String> {
        self.line_text_at(line)
    }

    /// Whether absolute grid `line` CONTINUES onto the next line (alacritty's `WRAPLINE` on its
    /// last cell) — the absolute-line analogue of [`row_wraps`](Self::row_wraps), for type-over
    /// across a soft-wrapped prompt regardless of scroll position.
    pub fn line_wraps(&self, line: i32) -> bool {
        self.wrap_at(line)
    }

    /// The cursor's row within the current viewport (display offset applied), or `None` when the
    /// cursor is scrolled out of view or the viewport sits in the compressed store. Cheap — reads
    /// the grid cursor directly, no snapshot. Used to scope type-over-selection to the live
    /// prompt line (the cursor's own row).
    pub fn cursor_row(&self) -> Option<usize> {
        self.screen_row(self.term.grid().cursor.point.line.0)
    }

    /// Every grid line — scrollback history included — as `(absolute_line, text)`, top to bottom.
    /// Absolute lines are negative for scrollback and `0..rows` for the live viewport (the same
    /// numbering the snapshot/cursor use). One char per cell, blanks as spaces. Used by the
    /// search model; O(history) so call it on query change, not per frame.
    pub fn history_lines(&self) -> Vec<(i32, String)> {
        let h = self.term.grid().history_size() as i32;
        let s = self.stored_rows() as i32;
        let top = -(s + h);
        let bottom = self.size.rows as i32 - 1;
        let mut out = Vec::with_capacity((bottom - top + 1).max(0) as usize);
        for line_i in top..=bottom {
            if let Some(text) = self.line_text_at(line_i) {
                out.push((line_i, text));
            }
        }
        out
    }

    /// Scroll so absolute grid `line` is visible (roughly centered), unless it already is. Marks
    /// the grid dirty when the offset actually moves. Used to reveal the active search match.
    pub fn scroll_to_visible(&mut self, line: i32) {
        if self.screen_row(line).is_some() {
            return;
        }
        let top =
            (self.visible_index(line) - self.size.rows as i32 / 2).clamp(0, self.visible_index(0));
        self.set_view_offset((-self.source_index(top)).max(0) as usize);
    }

    /// Pin the viewport back to the bottom (the live prompt), e.g. when search closes.
    pub fn scroll_to_bottom(&mut self) {
        self.set_view_offset(0);
    }

    /// Jump the viewport to the very top of the retained history (Shift+Home).
    pub fn scroll_to_top(&mut self) {
        self.set_view_offset(self.total_history());
    }

    /// Scroll the viewport by `delta_lines`: **positive scrolls up into history**, negative
    /// scrolls back toward the live edge. The virtual offset is clamped to the retained history
    /// bounds (store + alacritty window), so an over-scroll past the top/bottom is a no-op.
    /// Marks the grid dirty only when the offset actually moved. Drives mouse-wheel +
    /// Shift-PageUp/Down scrollback.
    pub fn scroll_by(&mut self, delta_lines: i32) {
        if delta_lines == 0 {
            return;
        }
        let before = self.viewport.offset();
        let top = self.visible_index(-(before as i32));
        let next = (i64::from(top) - i64::from(delta_lines))
            .clamp(0, i64::from(self.visible_index(0))) as i32;
        self.set_view_offset((-self.source_index(next)).max(0) as usize);
    }
}
