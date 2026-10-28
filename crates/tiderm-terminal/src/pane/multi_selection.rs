//! Pane-local, bounded selections. Only the active range changes on endpoint edits.
use super::TerminalPane;
use crate::selection::{Cell, Selection, SelectionKind};

pub(super) const MAX_SELECTIONS: usize = 32;

pub(super) struct SavedSelection {
    pub id: u64,
    pub range: Selection,
}

impl TerminalPane {
    pub fn multi_selection(&self) -> bool {
        self.multi_selection
    }

    pub fn set_multi_selection(&mut self, enabled: bool) {
        self.multi_selection = enabled;
    }

    pub fn selection_count(&self) -> usize {
        self.live_selections().count()
    }

    /// Stable pane-local ID and 1-based creation-order position; zero means no active range.
    pub fn selection_status(&self) -> (u64, usize, usize) {
        let mut ids: Vec<_> = self.live_selections().map(|(id, _)| id).collect();
        ids.sort_unstable();
        let position = ids.iter().position(|&id| id == self.selection_id);
        (
            position.map_or(0, |_| self.selection_id),
            position.map_or(0, |n| n + 1),
            ids.len(),
        )
    }

    pub(super) fn live_selections(&self) -> impl Iterator<Item = (u64, &Selection)> {
        self.saved_selections
            .iter()
            .map(|s| (s.id, &s.range))
            .chain(self.selection.as_ref().map(|s| (self.selection_id, s)))
            .filter(|(_, s)| s.dragged)
    }

    pub(super) fn next_selection_id(&mut self) {
        self.selection_serial = self.selection_serial.saturating_add(1);
        self.selection_id = self.selection_serial;
    }

    /// A Ctrl gesture (or the pane's append policy) preserves old ranges. Full sets reject
    /// the gesture without discarding anything. A stationary click restores the previous active.
    pub fn selection_begin_add(&mut self, x: f32, y: f32, w: f32, h: f32, column: bool) -> bool {
        let kind = if column {
            SelectionKind::Column
        } else {
            self.selection_kind
        };
        self.selection_start(x, y, w, h, kind, true)
    }

    pub(super) fn selection_start(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        kind: SelectionKind,
        append: bool,
    ) -> bool {
        if self.selection_locked {
            return false;
        }
        // Revealing folds clears source selections by policy, so do it before saving ranges.
        if kind == SelectionKind::Column {
            self.expand_column_folds();
        }
        let Some(cell) = self.cell_at_clamped(x, y, w, h) else {
            return false;
        };
        self.unfold_target(cell);
        let Some(cell) = self.cell_at_clamped(x, y, w, h) else {
            return false;
        };
        if append && self.selection_count() >= MAX_SELECTIONS {
            return false;
        }
        if append {
            if let Some(range) = self.selection.take().filter(|s| s.dragged) {
                self.saved_selections.push(SavedSelection {
                    id: self.selection_id,
                    range,
                });
            }
        } else {
            self.saved_selections.clear();
        }
        self.reading.visual = None;
        self.next_selection_id();
        self.selection = Some(Selection::with_kind(cell, kind));
        self.select_origin = Some((x, y));
        self.drag_pointer = Some((x, y, w, h));
        true
    }

    pub(super) fn selection_finish(&mut self) {
        if !self.has_selection() && !self.saved_selections.is_empty() {
            let saved = self.saved_selections.pop().expect("checked non-empty set");
            self.selection_id = saved.id;
            self.selection = Some(saved.range);
        }
        self.sync_active_reading();
    }

    pub(super) fn sync_active_reading(&mut self) {
        if self.reading.active
            && let Some(sel) = self.selection.filter(|s| s.dragged)
        {
            self.reading_cancel();
            self.reading.visual = Some(sel.anchor);
            self.reading.visual_column = sel.kind == SelectionKind::Column;
            self.reading.cursor = sel.head;
            self.reading_clamp(false);
        }
    }

    pub(super) fn reading_begin_visual(&mut self, column: bool) {
        if self.selection_locked {
            return;
        }
        let cursor = self.reading.cursor;
        if column {
            self.expand_column_folds();
        }
        self.unfold_target(cursor);
        if self.multi_selection {
            if self.selection_count() >= MAX_SELECTIONS {
                self.set_toast("At most 32 selections per pane");
                return;
            }
            if let Some(range) = self.selection.take().filter(|s| s.dragged) {
                self.saved_selections.push(SavedSelection {
                    id: self.selection_id,
                    range,
                });
            }
        } else {
            self.selection_clear();
        }
        self.reading.visual = Some(cursor);
        self.reading.visual_column = column;
        self.reading_select(cursor, cursor);
    }

    /// Cycle by creation order, wrapping at either end. Reveals the chosen head without writes.
    pub fn selection_cycle(&mut self, forward: bool) -> bool {
        self.end_selection_drag();
        let mut ids: Vec<_> = self.live_selections().map(|(id, _)| id).collect();
        ids.sort_unstable();
        if ids.len() < 2 {
            return false;
        }
        let at = ids
            .iter()
            .position(|&id| id == self.selection_id)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % ids.len()
        } else {
            (at + ids.len() - 1) % ids.len()
        };
        let Some(index) = self.saved_selections.iter().position(|s| s.id == ids[next]) else {
            return false;
        };
        let saved = self.saved_selections.remove(index);
        if let Some(range) = self.selection.replace(saved.range) {
            self.saved_selections.push(SavedSelection {
                id: self.selection_id,
                range,
            });
        }
        self.selection_id = saved.id;
        self.sync_active_reading();
        self.grid.scroll_to_visible(saved.range.head.line);
        true
    }

    /// Removes only the active highlight, never terminal text or shell input.
    pub fn selection_remove_active(&mut self) -> bool {
        if self.selection_locked {
            return false;
        }
        if !self.has_selection() {
            return false;
        }
        self.selection = None;
        self.reading.visual = None;
        self.end_selection_drag();
        true
    }

    /// Drop ranges whose endpoints are lost; surviving IDs and types remain unchanged.
    pub(super) fn remap_selections(
        &mut self,
        map: impl Fn(Cell) -> Option<Cell>,
        columns: bool,
    ) -> bool {
        let remap = |sel: &mut Selection| {
            if !columns && sel.kind == SelectionKind::Column {
                return false;
            }
            match (map(sel.anchor), map(sel.head)) {
                (Some(a), Some(h)) => {
                    sel.anchor = a;
                    sel.head = h;
                    true
                }
                _ => false,
            }
        };
        self.saved_selections.retain_mut(|s| remap(&mut s.range));
        if self.selection.as_mut().is_some_and(|s| !remap(s)) {
            self.selection = None;
            self.reading.visual = None;
            self.drag_pointer = None;
            self.select_origin = None;
            if let Some(saved) = self.saved_selections.pop() {
                self.selection_id = saved.id;
                self.selection = Some(saved.range);
            }
            if self.selection_count() == 0 {
                self.selection_locked = false;
            }
            return true;
        }
        if self.selection_count() == 0 {
            self.selection_locked = false;
        }
        false
    }
}
