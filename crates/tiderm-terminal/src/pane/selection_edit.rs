//! Endpoint editing: extend an existing selection to a new point, or swap its two ends.
//!
//! Both act on an **existing** selection instead of starting a new drag, so they deliberately skip
//! the [`DRAG_THRESHOLD_PX`](super::DRAG_THRESHOLD_PX) dead zone: "extend here" is an explicit act,
//! not a click twitch that must stay below the slop.
use super::TerminalPane;
use crate::selection::{Cell, SelectionKind};

impl TerminalPane {
    /// Widen `end` to cover a whole glyph when it lands on the leading cell of a wide/combining
    /// character while the selection heads forward. Keyboard and pointer aims share this so a
    /// select-to can never slice a wide cell in half where the equivalent motion does not.
    pub(super) fn widen_head(&self, start: Cell, end: Cell) -> Cell {
        if (end.line, end.col) < (start.line, start.col) {
            return end;
        }
        match self
            .grid
            .reading_cells(end.line)
            .iter()
            .find(|c| c.col == end.col)
        {
            Some(cell) => Cell {
                col: end.col + cell.width - 1,
                ..end
            },
            None => end,
        }
    }

    /// True when a live, already-dragged selection exists — its endpoints can be re-aimed.
    pub fn has_selection(&self) -> bool {
        self.selection.is_some_and(|s| s.dragged)
    }

    /// Move the selection's head to the (logical-px) point while keeping the anchor: the gesture
    /// behind "select to". `false` when there is nothing to extend, so the caller can fall back to
    /// a fresh drag instead of silently swallowing the click.
    ///
    /// Preserve the live selection's kind; `column` forces a rectangle for a modifier-held
    /// gesture, mirroring [`selection_begin_column`](Self::selection_begin_column).
    ///
    /// Folds are revealed first, matching [`selection_begin`](Self::selection_begin) — aiming into a
    /// collapsed block would otherwise resolve to a hidden source row the user cannot see.
    pub fn selection_extend_to(
        &mut self,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
        column: bool,
    ) -> bool {
        if self.selection_locked || !self.has_selection() {
            return false;
        }
        let Some(cell) = self.cell_at_clamped(x, y, surf_w, surf_h) else {
            return false;
        };
        self.unfold_target(cell);
        let (anchor, existing_kind) = match self.selection.as_ref() {
            Some(sel) => (sel.anchor, sel.kind),
            None => return false,
        };
        // A temporary Alt drag has its own kind, independent of the next drag's default.
        let kind = if column {
            SelectionKind::Column
        } else {
            existing_kind
        };
        let end = self.widen_head(anchor, cell);
        if let Some(sel) = self.selection.as_mut() {
            sel.kind = kind;
            sel.head = end;
            sel.dragged = true;
        }
        if self.reading.active && self.reading.visual.is_some() {
            self.reading.visual = Some(anchor);
            self.reading.visual_column = kind == SelectionKind::Column;
            self.reading.cursor = cell;
        }
        true
    }

    /// Swap the selection's anchor and head. The covered text stays byte-identical — only the
    /// direction flips — so the next [`selection_extend_to`](Self::selection_extend_to) grows from
    /// the opposite end. `false` when there is no selection to swap.
    pub fn selection_swap_anchor(&mut self) -> bool {
        if self.selection_locked {
            return false;
        }
        if !self.has_selection() {
            return false;
        }
        if self.reading.active && self.reading.visual.is_some() {
            return self.reading_swap_anchor();
        }
        match self.selection.as_mut() {
            Some(sel) => {
                std::mem::swap(&mut sel.anchor, &mut sel.head);
                true
            }
            None => false,
        }
    }

    /// Reveal a fold marker sitting on `cell` so endpoint edits never aim at a hidden row.
    pub(super) fn unfold_target(&mut self, cell: Cell) {
        if self.grid.fold_marker(cell.line).is_some() {
            self.command_unfold_line(cell.line);
        }
    }

    /// Swap the reading-mode anchor and cursor, then rebuild the selection between them so the
    /// highlight, the copy text and the keyboard cursor agree on the new direction.
    pub(super) fn reading_swap_anchor(&mut self) -> bool {
        if self.selection_locked {
            return false;
        }
        let Some(anchor) = self.reading.visual else {
            return false;
        };
        let cursor = self.reading.cursor;
        self.reading.visual = Some(cursor);
        self.reading.cursor = anchor;
        self.reading_select(cursor, anchor);
        true
    }
}
