//! Pane-local stream/column policy; endpoints remain bounded source coordinates.
use super::TerminalPane;
use crate::selection::SelectionKind;

impl TerminalPane {
    /// A modifier gesture selects columns once, without changing the pane's menu policy.
    pub fn selection_begin_column(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let saved = self.selection_kind;
        self.selection_kind = SelectionKind::Column;
        self.selection_begin(x, y, w, h);
        self.selection_kind = saved;
    }
    pub(super) fn expand_column_folds(&mut self) {
        if self.commands.has_folded() {
            self.command_fold_all(false);
        }
    }
    pub fn column_selection(&self) -> bool {
        self.selection_kind == SelectionKind::Column
    }
    pub fn set_column_selection(&mut self, column: bool) {
        if self.selection_locked {
            return;
        }
        if column {
            self.expand_column_folds();
        }
        if self.reading.visual.is_some() {
            self.reading.visual_column = column;
        }
        self.selection_kind = if column {
            SelectionKind::Column
        } else {
            SelectionKind::Stream
        };
        if let Some(selection) = &mut self.selection {
            selection.kind = self.selection_kind;
        }
    }
}
