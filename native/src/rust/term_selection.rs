//! Selection policy is owned by a pane, independent of its input/reading mode.
use super::term::{scale, try_with_tab};
use crate::bridge::selection::selection_ffi::{SelectionRect, SelectionStatus, SelectionText};

#[path = "term_selection_preview.rs"]
mod preview;
pub use preview::term_selection_preview;

pub fn term_column_selection(idx: u32) -> bool {
    try_with_tab(idx, |st| st.pane.column_selection()).unwrap_or(false)
}

pub fn term_set_column_selection(idx: u32, column: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        st.pane.set_column_selection(column);
        st.redraw_pending = true;
    })
    .is_some()
}

pub fn term_selection_begin_column(idx: u32, x: f32, y: f32) {
    if super::term_ops::input_locked() {
        return;
    }
    try_with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane.selection_begin_column(x * s, y * s, w * s, h * s);
        st.redraw_pending = true;
    });
}

pub fn term_selection_text(idx: u32) -> SelectionText {
    try_with_tab(idx, |st| match st.pane.selection_text_result() {
        Ok(text) => SelectionText {
            text: text.unwrap_or_default(),
            exceeds_budget: false,
        },
        Err(_) => SelectionText {
            text: String::new(),
            exceeds_budget: true,
        },
    })
    .unwrap_or_default()
}

/// Re-aim a live selection's head at the pointer, keeping its anchor. `false` when there is no
/// selection to extend, so the view can fall back to a fresh drag instead of dropping the press.
/// `column` forces the rectangle for a modifier-held gesture, like `term_selection_begin_column`.
pub fn term_selection_extend_to(idx: u32, x: f32, y: f32, column: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        let moved = st
            .pane
            .selection_extend_to(x * s, y * s, w * s, h * s, column);
        st.redraw_pending = true;
        moved
    })
    .unwrap_or(false)
}

/// Swap the selection's two ends: the covered text is unchanged, the next extension grows from the
/// opposite end. `false` when there is nothing to swap.
pub fn term_selection_swap_anchor(idx: u32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let swapped = st.pane.selection_swap_anchor();
        st.redraw_pending = true;
        swapped
    })
    .unwrap_or(false)
}

/// Whether the pane holds a live selection — gates the select-to / swap-anchor menu entries.
pub fn term_selection_active(idx: u32) -> bool {
    try_with_tab(idx, |st| st.pane.has_selection()).unwrap_or(false)
}

pub fn term_multi_selection(idx: u32) -> bool {
    try_with_tab(idx, |st| st.pane.multi_selection()).unwrap_or(false)
}

pub fn term_set_multi_selection(idx: u32, enabled: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| st.pane.set_multi_selection(enabled)).is_some()
}

pub fn term_selection_status(idx: u32) -> SelectionStatus {
    try_with_tab(idx, |st| {
        let (active_id, active, count) = st.pane.selection_status();
        SelectionStatus {
            active_id,
            active: active as u32,
            count: count as u32,
        }
    })
    .unwrap_or_default()
}

pub fn term_selection_begin_add(idx: u32, x: f32, y: f32, column: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        let added = st
            .pane
            .selection_begin_add(x * s, y * s, w * s, h * s, column);
        st.redraw_pending = true;
        added
    })
    .unwrap_or(false)
}

pub fn term_selection_cycle(idx: u32, forward: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let changed = st.pane.selection_cycle(forward);
        st.redraw_pending = true;
        changed
    })
    .unwrap_or(false)
}

pub fn term_selection_remove_active(idx: u32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let changed = st.pane.selection_remove_active();
        st.redraw_pending = true;
        changed
    })
    .unwrap_or(false)
}

pub fn term_selection_active_rects(idx: u32) -> Vec<SelectionRect> {
    try_with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane
            .selection_active_rects(w * s, h * s)
            .into_iter()
            .map(|(x, y, w, h)| SelectionRect {
                x: x / s,
                y: y / s,
                w: w / s,
                h: h / s,
            })
            .collect()
    })
    .unwrap_or_default()
}

pub fn term_selection_locked(idx: u32) -> bool {
    try_with_tab(idx, |st| st.pane.selection_locked()).unwrap_or(false)
}
pub fn term_set_selection_locked(idx: u32, locked: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let changed = st.pane.set_selection_locked(locked);
        st.redraw_pending |= changed;
        changed
    })
    .unwrap_or(false)
}
pub fn term_selection_transform(idx: u32, invert: bool) -> u8 {
    if super::term_ops::input_locked() {
        return 5;
    }
    try_with_tab(idx, |st| {
        let result = if invert {
            st.pane.selection_invert()
        } else {
            st.pane.selection_split()
        };
        st.redraw_pending |= result.is_ok();
        result.map_or_else(|error| error as u8, |()| 0)
    })
    .unwrap_or(4)
}
pub fn term_path_selection(idx: u32) -> bool {
    try_with_tab(idx, |st| st.config.meta.options.path_word_selection).unwrap_or(false)
}
pub fn term_set_path_selection(idx: u32, enabled: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        st.config.meta.options.path_word_selection = enabled
    })
    .is_some()
}
pub fn term_selection_token(idx: u32, x: f32, y: f32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        let changed = st.pane.selection_token_at(
            x * s,
            y * s,
            w * s,
            h * s,
            st.config.meta.options.path_word_selection,
        );
        st.redraw_pending |= changed;
        changed
    })
    .unwrap_or(false)
}
