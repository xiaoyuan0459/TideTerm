//! Fold definitions are pane-owned Rust state; Qt receives a bounded tree snapshot.
use super::term::{try_with_tab, TermState};
use crate::bridge::commands::commands_ffi::{FoldResult, FoldRow};

pub fn term_fold_rows(idx: u32) -> Vec<FoldRow> {
    if super::term_ops::input_locked() {
        return Vec::new();
    }
    try_with_tab(idx, |st| {
        st.pane
            .fold_regions()
            .into_iter()
            .map(|r| FoldRow {
                id: r.id,
                parent: r.parent,
                depth: r.depth as u32,
                title: r.title,
                first: r.first,
                last: r.last,
                folded: r.folded,
                manual: r.manual,
            })
            .collect()
    })
    .unwrap_or_default()
}

pub fn term_fold_count(idx: u32) -> u32 {
    if super::term_ops::input_locked() {
        return 0;
    }
    try_with_tab(idx, |st| st.pane.fold_region_count() as u32).unwrap_or(0)
}

pub fn term_fold_selection_status(idx: u32) -> u8 {
    if super::term_ops::input_locked() {
        return 6;
    }
    try_with_tab(idx, |st| {
        st.pane.fold_selection_status().err().map_or(0, |e| e as u8)
    })
    .unwrap_or(7)
}

fn changed(st: &mut TermState) {
    super::term_reading::set_reading(st, true);
    st.preview_revision = st.preview_revision.wrapping_add(1);
    st.redraw_pending = true;
}

pub fn term_fold_selection(idx: u32) -> FoldResult {
    if super::term_ops::input_locked() {
        return FoldResult { id: 0, error: 6 };
    }
    try_with_tab(idx, |st| match st.pane.fold_selection() {
        Ok(id) => {
            changed(st);
            st.pane.fold_region_jump(id);
            FoldResult { id, error: 0 }
        }
        Err(e) => FoldResult {
            id: 0,
            error: e as u8,
        },
    })
    .unwrap_or(FoldResult { id: 0, error: 7 })
}

pub fn term_fold_set(idx: u32, id: u64, folded: bool, recursive: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let ok = st.pane.fold_region_set(id, folded, recursive);
        if ok {
            changed(st);
        }
        ok
    })
    .unwrap_or(false)
}

pub fn term_fold_remove(idx: u32, id: u64) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        let ok = st.pane.fold_region_remove(id);
        if ok {
            changed(st);
        }
        ok
    })
    .unwrap_or(false)
}

pub fn term_fold_jump(idx: u32, id: u64) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| {
        if !st.pane.fold_regions().iter().any(|r| r.id == id) {
            return false;
        }
        changed(st);
        st.pane.fold_region_jump(id)
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::{term, term_diag, term_ops, term_reading, term_tests};
    #[test]
    fn live_fold_bridge_rejects_locked_and_retired_mutations_without_losing_ranges() {
        let _serial = term_tests::input_lock_guard();
        let tab = term::term_new_tab();
        let mut data = b"\x1b]133;A\x07$ \x1b]133;B\x07echo TREE\r\n\x1b]133;C\x07".to_vec();
        for row in 0..10 {
            data.extend_from_slice(format!("ROW_{row}\r\n").as_bytes());
        }
        data.extend_from_slice(b"\x1b]133;D;0\x07\x1b]133;A\x07$ ");
        term_diag::term_feed_inject(tab, &data);
        term_reading::term_set_reading(tab, true);
        for c in "gg2jv2j".chars() {
            term::term_key(tab, 0, 0, &c.to_string());
        }
        let created = term_fold_selection(tab);
        assert_eq!(created.error, 0);
        assert_ne!(created.id, 0);
        assert_eq!(term_fold_count(tab), 2);
        term_ops::term_set_locked(true);
        let rows = term_fold_rows(tab);
        let status = term_fold_selection_status(tab);
        let attempt = term_fold_selection(tab);
        let changed = term_fold_set(tab, created.id, false, true);
        let jumped = term_fold_jump(tab, created.id);
        let removed = term_fold_remove(tab, created.id);
        term_ops::term_set_locked(false);
        assert!(rows.is_empty() && status == 6 && attempt.error == 6);
        assert!(!changed && !jumped && !removed);
        assert!(
            term_fold_rows(tab)
                .iter()
                .find(|r| r.id == created.id)
                .unwrap()
                .folded
        );
        term::term_close_tab(tab);
        assert!(term_fold_rows(tab).is_empty());
        assert_eq!(term_fold_count(tab), 0);
        assert_eq!(term_fold_selection_status(tab), 7);
        assert_eq!(term_fold_selection(tab).error, 7);
        assert!(!term_fold_set(tab, created.id, false, false));
        assert!(!term_fold_jump(tab, created.id));
        assert!(!term_fold_remove(tab, created.id));
    }
}
