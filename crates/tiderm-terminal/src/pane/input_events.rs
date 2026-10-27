//! TerminalPane: input events.
use super::*;

impl TerminalPane {
    /// Pin the viewport back to the live edge (display offset 0) so the cursor is visible at the
    /// end of whatever was just written — e.g. after a paste, regardless of scrollback position.
    pub fn scroll_to_bottom(&mut self) {
        self.grid.scroll_to_bottom();
    }

    /// Scroll the scrollback viewport by `delta_lines` (positive = up into history, negative =
    /// toward the live edge), clamped to the history bounds. Stamps the scrollbar's show timer.
    pub fn scroll_by(&mut self, delta_lines: i32) {
        self.grid.scroll_by(delta_lines);
        self.scroll_activity = Some(Instant::now());
    }

    /// Scroll the scrollback viewport by one page (`up` = into history, else toward the live
    /// edge). A page is the visible row count less one row of overlap, so successive pages keep a
    /// line of context. Drives Shift+PageUp / Shift+PageDown.
    pub fn scroll_page(&mut self, up: bool) {
        let (_, rows) = self.grid_size();
        let page = (rows as i32 - 1).max(1);
        self.scroll_by(if up { page } else { -page });
    }

    /// Jump the viewport to the very top of scrollback (Shift+Home). Stamps the scrollbar timer.
    pub fn scroll_to_top(&mut self) {
        let (hist, _, off) = self.grid.scroll_metrics();
        if hist > off {
            self.scroll_by((hist - off) as i32);
        }
    }

    /// Handle a mouse-wheel notch over the pane. `delta_lines` is positive for wheel-up (into
    /// history) / negative for wheel-down, in scrollback lines (the widget sends ±3). Returns the
    /// bytes the controller should write to the pty when the wheel belongs to the **application**
    /// instead of our scrollback:
    ///
    /// * a **mouse-grabbing app** (DECSET 1000/1002/1003 — vim, htop, Claude Code) → a mouse-wheel
    ///   report at the pointer cell, so the app scrolls its own view;
    /// * the **alternate screen** with no mouse mode (less, man, a pager) → up/down arrow keys
    ///   (xterm's "alternate scroll"), so the pager scrolls a line at a time.
    ///
    /// Otherwise it scrolls our own scrollback viewport and returns `None`. This is the fix for
    /// "can't scroll Claude": in the alt screen there is no scrollback for `scroll_by` to move, so
    /// the wheel must be forwarded to the app.
    pub fn wheel(
        &mut self,
        delta_lines: i32,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<Vec<u8>> {
        if delta_lines == 0 {
            return None;
        }
        if self.grid.mouse_mode() {
            return self.mouse_wheel_report(delta_lines, x, y, surf_w, surf_h);
        }
        if self.grid.alt_screen() {
            return Some(alt_scroll_arrows(delta_lines, self.grid.app_cursor()));
        }
        self.scroll_by(delta_lines);
        None
    }

    /// Build mouse-wheel report bytes for a mouse-grabbing app: one report per wheel notch (a notch
    /// is [`WHEEL_LINES_PER_NOTCH`] of `delta_lines`), encoded SGR (`ESC[<Cb;Cx;Cy M`) when the app
    /// asked for it (DECSET 1006), else legacy X10 (`ESC[M` + 3 bytes). Button 64 = wheel up, 65 =
    /// down; the position is the 1-based cell under the pointer.
    /// 1-based `(col, row)` of the pointer at logical px `(x, y)` over a `surf_w`×`surf_h` surface,
    /// clamped into the grid. Shared by every mouse report.
    pub(super) fn cell_1based(
        &self,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<(usize, usize)> {
        match self.cell_logical(surf_w, surf_h) {
            Some((cw, ch, cols, rows)) => {
                // Gutter: strip sits left of the text — subtract before the column math.
                let x = x - self.gutter_cells as f32 * cw;
                let c = ((x.max(0.0)) / cw).floor().clamp(0.0, (cols - 1) as f32) as usize + 1;
                let r = (y / ch).floor().clamp(0.0, (rows - 1) as f32) as usize;
                Some((c, self.grid.mouse_row(r)? + 1))
            }
            None => Some((1, 1)),
        }
    }

    /// Encode ONE mouse report: `cb` is the button/event code (motion already includes the +32
    /// motion bit; wheel is 64/65), `release` picks SGR final `m`/X10 button-3. SGR (`ESC[<cb;c;r
    /// M|m`) when the app asked for it (DECSET 1006), else legacy X10 (`ESC[M` + 3 bytes, each +32).
    pub(super) fn fmt_mouse(&self, cb: u32, col: usize, row: usize, release: bool) -> Vec<u8> {
        if self.grid.sgr_mouse() {
            let term = if release { 'm' } else { 'M' };
            format!("\x1b[<{cb};{col};{row}{term}").into_bytes()
        } else {
            let b = if release { 3 } else { cb }; // X10 can't say which button released
            vec![
                0x1b,
                b'[',
                b'M',
                (b + 32).min(255) as u8,
                (col as u32 + 32).min(255) as u8,
                (row as u32 + 32).min(255) as u8,
            ]
        }
    }

    pub(super) fn mouse_wheel_report(
        &self,
        delta_lines: i32,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<Vec<u8>> {
        let (col, row) = self.cell_1based(x, y, surf_w, surf_h)?;
        let cb: u32 = if delta_lines > 0 { 64 } else { 65 };
        let notches = (delta_lines.unsigned_abs() / WHEEL_LINES_PER_NOTCH as u32).max(1);
        let mut out = Vec::new();
        for _ in 0..notches {
            out.extend_from_slice(&self.fmt_mouse(cb, col, row, false));
        }
        Some(out)
    }

    /// Whether the application has grabbed the mouse (DECSET 1000/1002/1003). When true the
    /// controller forwards button/drag/release events to the app (its own selection/clicks) instead
    /// of doing a local terminal selection — unless Shift is held (see [`mouse_report`]).
    ///
    /// [`mouse_report`]: Self::mouse_report
    pub fn app_grabs_mouse(&self) -> bool {
        self.grid.mouse_mode()
    }

    /// Bytes to forward to the pty for a pointer event over a mouse-grabbing app, or `None` to
    /// suppress it. `kind`: 0 = button press, 1 = motion, 2 = release. `button`: 0 = left, 1 =
    /// middle, 2 = right, 3 = none (a bare move). Motion is only reported when the app asked for it
    /// (1002 = while a button is held, 1003 = always); press/release always report. Returns `None`
    /// if the app isn't grabbing the mouse.
    pub fn mouse_report(
        &self,
        kind: i32,
        button: i32,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<Vec<u8>> {
        if !self.grid.mouse_mode() {
            return None;
        }
        let (col, row) = self.cell_1based(x, y, surf_w, surf_h)?;
        let btn = button.clamp(0, 3) as u32;
        match kind {
            0 => Some(self.fmt_mouse(btn, col, row, false)), // press
            2 => Some(self.fmt_mouse(btn.min(2), col, row, true)), // release
            1 => {
                // Motion: forward on any-motion (1003), or on drag (1002) only while a button is held.
                let held = button != 3;
                let want = self.grid.mouse_any_motion() || (self.grid.mouse_drag() && held);
                if !want {
                    return None;
                }
                let mb = if held { btn } else { 3 };
                Some(self.fmt_mouse(32 + mb, col, row, false))
            }
            _ => None,
        }
    }
}
