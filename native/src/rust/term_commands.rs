//! Command positions belong to the retained terminal buffer, not string history.
use super::term::try_with_tab;
use crate::bridge::commands::commands_ffi::{CommandRow, CommandText};

pub fn term_command_at_point(idx: u32, x: f32, y: f32) -> u64 {
    if super::term_ops::input_locked() {
        return 0;
    }
    try_with_tab(idx, |st| {
        let scale = super::term::scale();
        st.pane
            .command_at_point(
                x * scale,
                y * scale,
                st.viewport_px.0 * scale,
                st.viewport_px.1 * scale,
            )
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

pub fn term_command_text(idx: u32, id: u64) -> CommandText {
    if super::term_ops::input_locked() {
        return CommandText::default();
    }
    term_commands(idx)
        .into_iter()
        .find(|row| row.id == id && !row.startup)
        .map(|row| CommandText {
            available: true,
            text: row.title,
            truncated: false,
        })
        .unwrap_or_default()
}

pub fn term_command_select_block(idx: u32, id: u64) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        if !st
            .pane
            .command_blocks()
            .iter()
            .any(|row| row.id == id && !row.startup)
        {
            return false;
        }
        super::term_reading::set_reading(st, true);
        let selected = st.pane.command_select_block(id);
        st.redraw_pending |= selected;
        selected
    })
    .unwrap_or(false)
}

pub fn term_fold_markers(idx: u32) -> Vec<crate::bridge::commands::commands_ffi::FoldMarker> {
    if super::term_ops::input_locked() {
        return Vec::new();
    }
    try_with_tab(idx, |st| {
        if !st.opts.gutter {
            return Vec::new();
        }
        let (cw, ch) = super::term::font_metrics();
        let scale = super::term::scale();
        let x = st.pane.gutter_cells(true).saturating_sub(1) as f32 * cw as f32 / scale;
        st.pane
            .command_fold_markers()
            .into_iter()
            .map(
                |(id, row, folded)| crate::bridge::commands::commands_ffi::FoldMarker {
                    id,
                    x,
                    y: row as f32 * ch as f32 / scale,
                    w: cw as f32 / scale,
                    h: ch as f32 / scale,
                    folded,
                },
            )
            .collect()
    })
    .unwrap_or_default()
}

pub fn term_commands(idx: u32) -> Vec<CommandRow> {
    try_with_tab(idx, |st| {
        st.pane
            .command_blocks()
            .into_iter()
            .map(|b| CommandRow {
                id: b.id,
                title: b.title,
                line: b.line,
                finished: b.finished,
                folded: b.folded,
                foldable: b.foldable,
                startup: b.startup,
            })
            .collect()
    })
    .unwrap_or_default()
}

pub fn term_command_output(idx: u32, id: u64, preview: bool) -> CommandText {
    try_with_tab(idx, |st| {
        st.pane
            .command_output(id, preview)
            .map(|(text, truncated)| CommandText {
                available: true,
                text,
                truncated,
            })
            .unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn term_command_jump(idx: u32, id: u64) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        if !st.pane.command_blocks().iter().any(|b| b.id == id) {
            return false;
        }
        super::term_reading::set_reading(st, true);
        let moved = st.pane.command_jump(id);
        st.redraw_pending |= moved;
        moved
    })
    .unwrap_or(false)
}

pub fn term_command_step(idx: u32, forward: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        if st.pane.command_blocks().is_empty() {
            return false;
        }
        super::term_reading::set_reading(st, true);
        let moved = st.pane.command_step(forward);
        st.redraw_pending |= moved;
        moved
    })
    .unwrap_or(false)
}

pub fn term_command_fold(idx: u32, id: u64, folded: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        if !st.pane.command_set_folded(id, folded) {
            return false;
        }
        super::term_reading::set_reading(st, true);
        st.pane.command_jump(id);
        st.preview_revision = st.preview_revision.wrapping_add(1);
        st.redraw_pending = true;
        true
    })
    .unwrap_or(false)
}

pub fn term_command_fold_current(idx: u32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let changed = st.pane.command_fold_current(None);
        if changed {
            st.preview_revision = st.preview_revision.wrapping_add(1);
            super::term_reading::set_reading(st, true);
            st.redraw_pending = true;
        }
        changed
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_fold_controls_reject_locked_and_retired_sessions() {
        let _serial = crate::rust::term_tests::input_lock_guard();
        let tab = crate::rust::term::term_new_tab();
        crate::rust::term_lifecycle::term_disconnect_tab(tab);
        super::super::term::with_tab(tab, |st| {
            st.pane.clear();
            st.pane.command_startup_begin();
            st.opts.gutter = true;
            st.pane
                .feed("banner\r\nfixture copyright\r\n\r\n\x1b]133;A\x07$ ");
        });
        let markers = term_fold_markers(tab);
        assert_eq!(markers.len(), 1);
        let id = markers[0].id;
        crate::rust::term_ops::term_set_locked(true);
        let hidden = term_fold_markers(tab);
        let denied = term_command_fold(tab, id, true);
        crate::rust::term_ops::term_set_locked(false);
        assert!(hidden.is_empty() && !denied);
        assert!(term_command_fold(tab, id, true));
        crate::rust::term::term_close_tab(tab);
        assert!(term_fold_markers(tab).is_empty());
        assert!(!term_command_fold(tab, id, false));
    }
}
