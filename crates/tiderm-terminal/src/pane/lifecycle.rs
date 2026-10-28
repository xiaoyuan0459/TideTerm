//! TerminalPane: lifecycle.
use super::*;

impl TerminalPane {
    /// Feed a chunk of session output (the `data` of a `SessionEvent::Data`) into the grid.
    pub fn feed(&mut self, data: &str) {
        self.feed_viewport(data.as_bytes());
        self.process_feed_events();
    }

    /// Feed raw output bytes (when you have bytes rather than a decoded `String`).
    pub fn feed_bytes(&mut self, bytes: &[u8]) {
        self.feed_viewport(bytes);
        self.process_feed_events();
    }

    /// Act on OSC 52 clipboard requests parsed during the last feed (G6): store puts the
    /// text on the system clipboard (remote vim/tmux → local clipboard over ssh); load
    /// reads it and replies through the pty queue — see [`crate::grid::OscEvent`].
    pub(super) fn process_feed_events(&mut self) {
        // G8: BEL → transient "Bell" toast (visual bell; no audible beep from the renderer).
        if self.grid.take_bell() {
            self.set_toast("Bell");
        }
        for ev in self.grid.drain_osc_events() {
            match ev {
                crate::grid::OscEvent::ClipboardStore(text) => {
                    let n = text.chars().count();
                    if self.clipboard.copy(&text) {
                        self.set_toast(format!(
                            "Copied {n} char{} to clipboard",
                            if n == 1 { "" } else { "s" }
                        ));
                    }
                }
                crate::grid::OscEvent::ClipboardLoad(format) => {
                    let text = self.clipboard.paste().unwrap_or_default();
                    self.grid.reply_to_pty(format(&text).into_bytes());
                }
            }
        }
    }

    /// Drain terminal-originated replies (DSR/DA/etc.) that must be written back to the
    /// session's pty. Empty when there is nothing to forward. **Must** be forwarded or a
    /// real conpty blocks at startup (see [`TermGrid::take_replies`]).
    pub fn take_replies(&mut self) -> Vec<u8> {
        self.grid.take_replies()
    }

    /// Drain OSC 0;2 window-title updates reported by the running app. `None` when
    /// nothing arrived since the last call — the controller drains this every frame and
    /// overrides the pane header / tab label (falls back to the session name otherwise).
    pub fn take_title(&mut self) -> Option<String> {
        self.grid.take_title()
    }

    /// The caret position in viewport cells: `(col, row)`, or `None` when the cursor is
    /// scrolled out of view. Windows IME candidate-window anchoring (G2) — see
    /// `native/src/ime.rs`.
    /// The current input line (cursor row text) when the cursor is at its end — the
    /// CommandSuggest (G15) trigger condition. `None` when mid-line editing, the row is
    /// empty, or the cursor is scrolled out of view.
    pub fn input_line(&self) -> Option<String> {
        if self.grid.cursor_row().is_none() || !self.grid.cursor_at_line_end() {
            return None;
        }
        let trimmed = self.grid.cursor_line_text().trim_end().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub fn caret_cell(&self) -> Option<(usize, usize)> {
        if self.reading.active {
            return self.reading_caret_cell();
        }
        Some((self.grid.cursor_col(), self.grid.cursor_row()?))
    }

    /// Take-and-clear the repaint flag. `true` means the grid changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        self.grid.take_dirty()
    }
}
