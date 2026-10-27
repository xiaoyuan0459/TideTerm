//! TerminalPane: clipboard.
use super::*;

impl TerminalPane {
    // ---- Text selection ---------------------------------------------------------------------
    //
    // Drag to select a cell range; the controller turns it into highlight rects (for the Slint
    // overlay) and, on release, the selected text (copy-on-select — see `copy_selection`). The
    // model is our own (`crate::selection`) rather than alacritty's, kept in viewport cells.

    /// Copy the current selection to the system clipboard and raise a "Copied …" indicator
    /// (the copy-on-select behavior, also bound to Ctrl+C / Ctrl+Shift+C). Returns the number of
    /// characters copied, or `None` if there was no selection or the clipboard was unavailable.
    pub fn copy_selection(&mut self) -> Option<usize> {
        let text = match self.selection_text_result() {
            Ok(Some(text)) => text,
            Ok(None) => return None,
            Err(_) => {
                self.set_toast("Selection exceeds the 1 MiB copy budget");
                return None;
            }
        };
        let n = text.chars().count();
        if self.clipboard.copy(&text) {
            self.set_toast(format!(
                "Copied {} char{} to clipboard",
                n,
                if n == 1 { "" } else { "s" }
            ));
            Some(n)
        } else {
            None
        }
    }

    /// Copy arbitrary `text` (a Ctrl+clicked link/path) to the system clipboard and raise the
    /// "Copied …" indicator — the same injected host service + toast as
    /// [`copy_selection`](Self::copy_selection). Replaces the app's `clip.exe` shell-out, which
    /// blocked the UI thread on `child.wait()` for every Ctrl+click (a visible freeze) and
    /// showed no indicator.
    pub fn copy_text(&mut self, text: &str) -> bool {
        if self.clipboard.copy(text) {
            self.set_toast("Copied to clipboard");
            true
        } else {
            false
        }
    }

    /// Read the system clipboard for a right-click / Ctrl+V paste, raising a "Pasted …"
    /// indicator. Returns the text the caller should write to this pane's session (the controller
    /// doesn't own the session transport), or `None` when the clipboard is empty/unavailable.
    pub fn paste_from_clipboard(&mut self) -> Option<String> {
        let text = self.clipboard.paste()?;
        let n = text.chars().count();
        self.set_toast(format!(
            "Pasted {} char{}",
            n,
            if n == 1 { "" } else { "s" }
        ));
        Some(prepare_paste(&text, self.grid.bracketed_paste()))
    }

    /// Whether the OS clipboard holds an image (vs text). The controller uses this to decide
    /// whether a Ctrl+V with no clipboard text should forward a literal 0x16 to an in-pane TUI
    /// (Claude Code) that reads the clipboard image itself — see [`Clipboard::has_image`].
    pub fn clipboard_has_image(&mut self) -> bool {
        self.clipboard.has_image()
    }
}
