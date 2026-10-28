//! TerminalPane: feedback.
use super::*;

impl TerminalPane {
    // ---- scroll overlays (vim scrollbar + jump-to-bottom HUD) --------------------------------

    /// How far the viewport is scrolled up from the live edge, in lines (0 = pinned to the bottom).
    /// Drives the jump-to-bottom HUD: shown whenever this is non-zero.
    pub fn scroll_offset(&self) -> usize {
        self.grid.scroll_metrics().2
    }

    /// The vim-style scrollbar to draw right now, or `None` when there is no scrollback or the bar
    /// has fully faded. Returns `(thumb_y, thumb_h, opacity)` in logical px over a `surf_h`-tall
    /// pane: the thumb height is proportional to the visible fraction of the buffer, its position to
    /// how far down the buffer the viewport sits, and the opacity ramps from 1 down to 0 over the
    /// show-then-fade window since the last scroll gesture (so the bar is invisible while idle).
    pub fn scrollbar(&self, surf_h: f32) -> Option<(f32, f32, f32)> {
        let opacity = self.scrollbar_opacity()?;
        let (hist, rows, off) = self.grid.scroll_metrics();
        if hist == 0 || rows == 0 || surf_h <= 0.0 {
            return None;
        }
        let total = (hist + rows) as f32;
        let thumb_h = (surf_h * rows as f32 / total)
            .max(SCROLLBAR_MIN_THUMB_PX)
            .min(surf_h);
        // Fraction of the way down the buffer the viewport TOP sits: 0 at the very top of history,
        // 1 at the live edge. `hist - off` lines sit above the viewport top, out of `hist` total.
        let frac = if hist == 0 {
            1.0
        } else {
            (hist - off) as f32 / hist as f32
        };
        let thumb_y = (surf_h - thumb_h) * frac;
        Some((thumb_y, thumb_h, opacity))
    }

    /// Current scrollbar opacity from the show-then-fade timer, or `None` once it has fully faded
    /// (so the projection can drop the bar entirely while idle).
    pub(super) fn scrollbar_opacity(&self) -> Option<f32> {
        let e = self.scroll_activity?.elapsed().as_millis();
        if e < SCROLLBAR_SHOW_MS {
            Some(1.0)
        } else if e < SCROLLBAR_SHOW_MS + SCROLLBAR_FADE_MS {
            Some(1.0 - (e - SCROLLBAR_SHOW_MS) as f32 / SCROLLBAR_FADE_MS as f32)
        } else {
            None
        }
    }

    // ---- Copy/paste indicator ("toast") -----------------------------------------------------

    /// Raise a transient indicator over the pane (e.g. "Copied 12 chars to clipboard"). It
    /// auto-expires after [`TOAST_MS`]; poll it each frame with [`toast_text`](Self::toast_text).
    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// The indicator text to display right now, or `None` once it has expired (which also clears
    /// it). Call this every frame and push the result to the pane's `toast` property.
    pub fn toast_text(&mut self) -> Option<String> {
        let expired = match &self.toast {
            Some((_, at)) => at.elapsed().as_millis() >= TOAST_MS,
            None => return None,
        };
        if expired {
            self.toast = None;
            return None;
        }
        self.toast.as_ref().map(|(m, _)| m.clone())
    }
}
