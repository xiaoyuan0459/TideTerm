//! Pane-owned mark rules; native owns profile IO and lock/lifetime admission.
use super::term::{font_metrics, scale, try_with_tab, TermState};
use crate::bridge::marks::marks_ffi::{MarkRect, MarkResult, MarkRow, MarkSeed};
use tiderm_terminal::{
    marks::{MarkError, MarkSpec},
    search::SearchOptions,
};

fn result(value: Result<u64, MarkError>) -> MarkResult {
    match value {
        Ok(id) => MarkResult { id, error: 0 },
        Err(e) => MarkResult {
            id: 0,
            error: e as u8,
        },
    }
}
fn change(tab: u32, f: impl FnOnce(&mut TermState) -> Result<u64, MarkError>) -> MarkResult {
    if super::term_ops::input_locked() {
        return MarkResult { id: 0, error: 6 };
    }
    try_with_tab(tab, |st| {
        let reply = result(f(st));
        if reply.error == 0 {
            st.redraw_pending = true;
        }
        reply
    })
    .unwrap_or(MarkResult { id: 0, error: 7 })
}
pub fn term_mark_rows(tab: u32) -> Vec<MarkRow> {
    if super::term_ops::input_locked() {
        return Vec::new();
    }
    try_with_tab(tab, |st| {
        st.pane
            .mark_rules()
            .into_iter()
            .map(|r| MarkRow {
                id: r.id,
                pattern: r.spec.pattern,
                flags: r.spec.options.to_flags(),
                color: r.spec.color,
                enabled: r.spec.enabled,
            })
            .collect()
    })
    .unwrap_or_default()
}
pub fn term_mark_seed(tab: u32, selection_only: bool) -> MarkSeed {
    if super::term_ops::input_locked() {
        return MarkSeed {
            text: String::new(),
            error: 6,
        };
    }
    try_with_tab(tab, |st| match st.pane.mark_seed(selection_only) {
        Ok(text) => MarkSeed { text, error: 0 },
        Err(e) => MarkSeed {
            text: String::new(),
            error: e as u8,
        },
    })
    .unwrap_or(MarkSeed {
        text: String::new(),
        error: 7,
    })
}
pub fn term_mark_put(
    tab: u32,
    id: u64,
    pattern: &str,
    flags: u32,
    color: u8,
    enabled: bool,
) -> MarkResult {
    change(tab, |st| {
        if flags > 7 {
            return Err(MarkError::Pattern);
        }
        st.pane.mark_put(
            id,
            MarkSpec {
                pattern: pattern.into(),
                options: SearchOptions::from_flags(flags),
                color,
                enabled,
            },
        )
    })
}
pub fn term_mark_remove(tab: u32, id: u64) -> MarkResult {
    change(tab, |st| {
        if st.pane.mark_remove(id) {
            Ok(id)
        } else {
            Err(MarkError::Missing)
        }
    })
}
pub fn term_mark_clear(tab: u32) -> MarkResult {
    change(tab, |st| {
        st.pane.mark_clear();
        Ok(0)
    })
}
pub fn term_mark_move(tab: u32, id: u64, up: bool) -> MarkResult {
    change(tab, |st| {
        if st.pane.mark_move(id, up) {
            Ok(id)
        } else {
            Err(MarkError::Missing)
        }
    })
}
pub fn term_mark_quick(tab: u32, color: u8, remove: bool) -> MarkResult {
    change(tab, |st| st.pane.mark_quick(color, remove))
}
pub fn term_mark_step(tab: u32, id: u64, forward: bool) -> MarkResult {
    change(tab, |st| {
        super::term_reading::set_reading(st, true);
        st.pane.mark_step(id, forward)?;
        Ok(id)
    })
}
pub fn term_find_selection(tab: u32, forward: bool) -> MarkResult {
    change(tab, |st| {
        st.pane.search_selection(forward)?;
        Ok(0)
    })
}
pub fn term_mark_defaults(tab: u32, save: bool) -> MarkResult {
    if super::term_ops::input_locked() {
        return MarkResult { id: 0, error: 6 };
    }
    try_with_tab(tab, |st| {
        let ok = if save {
            let rules = super::mark_persistence::specs(st);
            super::mark_profile::save(&rules).map(|()| st.mark_baseline = Some(rules))
        } else {
            super::mark_profile::load().and_then(|rules| {
                st.pane
                    .mark_replace(&rules)
                    .map_err(|_| "invalid mark rules".to_string())?;
                st.mark_baseline = Some(rules);
                Ok(())
            })
        };
        if ok.is_ok() {
            st.redraw_pending = true;
        }
        MarkResult {
            id: 0,
            error: if ok.is_ok() { 0 } else { 8 },
        }
    })
    .unwrap_or(MarkResult { id: 0, error: 7 })
}
pub fn term_mark_rects(tab: u32) -> Vec<MarkRect> {
    if super::term_ops::input_locked() {
        return Vec::new();
    }
    try_with_tab(tab, |st| {
        let (cw, ch) = font_metrics();
        let s = scale();
        let gx = st.pane.gutter_cells(st.opts.gutter) as u32 * cw;
        st.pane
            .mark_cells()
            .into_iter()
            .map(|r| MarkRect {
                x: (gx + r.start as u32 * cw) as f32 / s,
                y: r.row as f32 * ch as f32 / s,
                w: (r.end - r.start) as f32 * cw as f32 / s,
                h: ch as f32 / s,
                color: r.color,
            })
            .collect()
    })
    .unwrap_or_default()
}
