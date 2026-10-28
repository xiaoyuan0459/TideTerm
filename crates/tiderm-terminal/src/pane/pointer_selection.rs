//! Pointer selection and guarded prompt-line edits.
use super::*;

impl TerminalPane {
    /// Begin a drag-selection anchored at the (logical-px) press point. Replaces any prior
    /// selection. The selection only starts *rendering* once the drag leaves the anchor cell, so
    /// a click that doesn't move still falls through to a link activation.
    pub fn selection_begin(&mut self, x: f32, y: f32, surf_w: f32, surf_h: f32) {
        self.selection_start(x, y, surf_w, surf_h, self.selection_kind, false);
    }

    /// Extend the active selection's head to the (logical-px) cursor point during a drag.
    ///
    /// Gated on [`DRAG_THRESHOLD_PX`]: until the pointer has moved that far from the press point
    /// the head stays pinned to the anchor (so the selection never becomes `dragged` and a click
    /// can't copy-on-select). This dead zone is what stops a stray click twitch — especially one
    /// that straddles a cell boundary — from clobbering the clipboard ahead of a paste.
    pub fn selection_update(&mut self, x: f32, y: f32, surf_w: f32, surf_h: f32) {
        if self.selection_locked {
            return;
        }
        // Track the live pointer for edge-autoscroll even below the drag threshold (autoscroll
        // itself only kicks in once the selection is actually dragged).
        self.drag_pointer = Some((x, y, surf_w, surf_h));
        if let Some((ox, oy)) = self.select_origin
            && (x - ox).hypot(y - oy) < DRAG_THRESHOLD_PX
        {
            return;
        }
        let c = match self.cell_at_clamped(x, y, surf_w, surf_h) {
            Some(c) => c,
            None => return,
        };
        if let Some(sel) = self.selection.as_mut() {
            sel.update(c);
        }
    }

    /// Drop the current selection (and its highlight).
    pub fn selection_clear(&mut self) {
        self.selection_locked = false;
        self.selection = None;
        self.saved_selections.clear();
        self.reading.visual = None;
        self.select_origin = None;
        self.drag_pointer = None;
    }

    /// End the drag (button released) — stops edge-autoscroll while KEEPING any selection (the
    /// controller may still copy it). Call from the `selection-end` handler.
    pub fn end_selection_drag(&mut self) {
        self.drag_pointer = None;
        self.select_origin = None;
        self.selection_finish();
    }

    /// One edge-autoscroll step while a selection drag is held in the top/bottom edge band: scroll
    /// the viewport one line toward that edge and extend the selection head to the just-revealed
    /// edge row, so the selection grows into off-screen scrollback (the vim/iTerm/Claude drag
    /// behavior). Returns `true` if it scrolled — the pump uses that to keep ticking + repainting.
    /// No-op unless a real (dragged) selection is in flight with the pointer at an edge.
    pub fn selection_autoscroll_tick(&mut self) -> bool {
        let (x, y, sw, sh) = match self.drag_pointer {
            Some(d) => d,
            None => return false,
        };
        if !self.selection.is_some_and(|s| s.dragged) {
            return false;
        }
        let (_, cell_h, _, _) = match self.cell_logical(sw, sh) {
            Some(t) => t,
            None => return false,
        };
        let edge = cell_h.max(8.0); // a one-row band (min 8px) at each edge
        let dir = if y < edge {
            1 // top edge → scroll up into history
        } else if y > sh - edge {
            -1 // bottom edge → scroll down toward the live edge
        } else {
            return false;
        };
        let before = self.grid.display_offset();
        self.scroll_by(dir);
        if self.grid.display_offset() == before {
            return false; // clamped at the top/bottom of the buffer
        }
        // Re-map the head to the edge row at the NEW offset → the line just scrolled into view.
        let edge_y = if dir > 0 { 0.0 } else { sh };
        if let Some(c) = self.cell_at_clamped(x, edge_y, sw, sh)
            && let Some(sel) = self.selection.as_mut()
        {
            sel.update(c);
        }
        true
    }

    /// Select the entire viewport (every visible cell), marked `dragged` so it renders and is
    /// copyable. This is the context menu's "Select All" — viewport-scoped (the region
    /// [`selection_text`](Self::selection_text) can reconstruct), mirroring xterm's `selectAll`
    /// over the on-screen buffer. A subsequent [`copy_selection`](Self::copy_selection) copies it.
    pub fn select_all(&mut self) {
        if self.selection_locked {
            return;
        }
        self.selection_clear();
        self.next_selection_id();
        let (cols, rows) = self.grid_size();
        if cols == 0 || rows == 0 {
            self.selection = None;
            return;
        }
        // The visible viewport, in absolute lines: top viewport row 0 is line `-offset`.
        let mut sel = Selection::new(selection::Cell {
            col: 0,
            line: self.grid.source_row(0).unwrap_or(0),
        });
        sel.update(selection::Cell {
            col: cols - 1,
            line: self
                .grid
                .source_row(rows - 1)
                .unwrap_or(self.grid.reading_rows().1),
        });
        self.selection = Some(sel);
    }

    /// True once the active selection has actually been dragged across cells (i.e. it's a real
    /// selection, not a stationary click). The caller uses this to choose copy-vs-click on release.
    pub fn selection_is_drag(&self) -> bool {
        self.selection.is_some_and(|s| s.dragged)
    }

    /// True when there's an active *dragged* selection lying entirely on the cursor's own viewport
    /// row — i.e. over the live shell input line, the only row a terminal can safely treat as
    /// editable text. Scopes type-over-selection to the prompt line: typing over a selection here
    /// drops the highlight (you're replacing your own input), whereas a selection on any other row
    /// (scrollback / command output) isn't in the shell's buffer and is left untouched, so no
    /// speculative deletes are ever sent (no PTY corruption). False for no selection, a non-dragged
    /// click, a multi-row span, or when the cursor is scrolled out of view.
    pub fn selection_on_cursor_row(&self) -> bool {
        if self.selection_count() > 1 {
            return false;
        }
        let sel = match &self.selection {
            Some(s) if s.dragged && s.kind == selection::SelectionKind::Stream => s,
            _ => return false,
        };
        let (start, end) = sel.ordered();
        if start.line != end.line {
            return false; // a multi-row selection is never a single prompt line
        }
        // Compare source coordinates after viewport folding and scrolling.
        match self.grid.cursor_row() {
            Some(crow) => self.grid.source_row(crow) == Some(start.line),
            None => false,
        }
    }

    /// Type-over selection: the byte sequence that ERASES the selected prompt-line text, to be
    /// written to the pty *before* a printable keystroke so the key replaces the selection.
    /// `None` (and no state change) unless the dragged selection lies on the cursor's own
    /// logical line — its visual row, or any rows soft-WRAPPED into it (a long shell input
    /// spanning several visual rows is still one editable line) — and the main screen is
    /// active; on `Some` the selection is also cleared.
    ///
    /// Safety model — the sequence is built ONLY from edit keys the line editor **clamps at the
    /// input-region boundaries** (left/right arrows, backspace, forward-delete are no-ops at the
    /// edges in PSReadLine/readline): if the selection overlaps the prompt decoration itself, the
    /// surplus moves/deletes simply do nothing — only the selected chars inside the editable
    /// input are removed, and nothing left of the input start can ever be touched. The
    /// alternate screen is excluded ([`TermGrid::alt_screen`](crate::grid::TermGrid::alt_screen)):
    /// there these bytes would be app commands (vim motions), not edits.
    ///
    /// Cell↔char caveat: counts are in grid cells, exact for ASCII (same wide-glyph caveat as
    /// [`selection_text`](Self::selection_text)).
    pub fn type_over_selection(&mut self) -> Option<Vec<u8>> {
        if self.selection_locked {
            return None;
        }
        if self.selection_count() > 1
            || self.grid.alt_screen()
            || self
                .selection
                .is_some_and(|s| s.kind == selection::SelectionKind::Column)
        {
            return None;
        }
        let sel = match &self.selection {
            Some(s) if s.dragged => s,
            _ => return None,
        };
        let (start, end) = sel.ordered();
        let crow = self.grid.cursor_row()?;
        let cline = self.grid.source_row(crow)?;
        let (cols, _) = self.grid_size();
        if cols == 0 {
            return None;
        }
        // The selection must lie on the SAME WRAPPED LOGICAL LINE as the cursor: every grid line
        // between the selection and the cursor (inclusive bounds, exclusive of the last) must carry
        // the WRAPLINE continuation flag. A single-line selection on the cursor's own line is the
        // degenerate case (empty range). This is what makes a long soft-wrapped shell input
        // editable across its visual rows, while a selection on a genuinely different line
        // (scrollback, command output) still declines.
        let lo = start.line.min(cline);
        let hi = end.line.max(cline);
        if (lo..hi).any(|r| !self.grid.line_wraps(r)) {
            return None;
        }
        // Linear cell offsets within the wrapped line: a wrapped row is always `cols` cells wide,
        // and the line editor's arrows/backspace walk straight through the wrap, so the single-row
        // arithmetic below holds verbatim in linear space. Absolute line numbers are fine — only
        // the differences between these offsets matter.
        let lin = |line: i32, col: usize| line * cols as i32 + col as i32;
        let s = lin(start.line, start.col);
        let e = lin(end.line, end.col);
        let c = lin(cline, self.grid.cursor_col());
        const LEFT: &[u8] = b"\x1b[D";
        const RIGHT: &[u8] = b"\x1b[C";
        const BS: &[u8] = &[0x7f];
        const FDEL: &[u8] = b"\x1b[3~"; // forward delete (DeleteChar)
        let mut bytes = Vec::new();
        let mut rep = |seq: &[u8], n: i32| {
            for _ in 0..n.max(0) {
                bytes.extend_from_slice(seq);
            }
        };
        if c > e {
            // Caret right of the selection (the common case — you selected text you just
            // typed): step left to the selection end, then backspace it away.
            rep(LEFT, c - (e + 1));
            rep(BS, e - s + 1);
        } else if c <= s {
            // Caret left of (or at) the selection: step right to its start, forward-delete it.
            rep(RIGHT, s - c);
            rep(FDEL, e - s + 1);
        } else {
            // Caret inside the selection: backspace the left part, forward-delete the rest.
            rep(BS, c - s);
            rep(FDEL, e - c + 1);
        }
        self.selection_clear();
        Some(bytes)
    }
}
