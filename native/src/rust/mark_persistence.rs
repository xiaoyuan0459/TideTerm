//! Explicit close-time persistence, separate from transport/session retirement.
use super::{
    mark_profile,
    term::{try_with_tab, TermState, TABS},
};
use crate::bridge::marks::marks_ffi::MarkResult;
use tiderm_terminal::marks::MarkSpec;

pub(super) fn specs(st: &TermState) -> Vec<MarkSpec> {
    st.pane.mark_rules().into_iter().map(|r| r.spec).collect()
}
fn reply(error: u8) -> MarkResult {
    MarkResult { id: 0, error }
}

pub fn term_mark_set_save_on_close(tab: u32, enabled: bool) -> MarkResult {
    if super::term_ops::input_locked() {
        return reply(6);
    }
    try_with_tab(tab, |st| {
        st.config.meta.options.mark_save_on_close = enabled;
        reply(0)
    })
    .unwrap_or(reply(7))
}

/// All requested panes are checked before a single shared-default write.
/// A rejected close must leave every live pane and its unsaved rules available.
pub fn term_mark_prepare_close(ids: &[u32]) -> MarkResult {
    TABS.with(|tabs| {
        let mut tabs = tabs.borrow_mut();
        let mut wanted: Option<Vec<MarkSpec>> = None;
        for id in ids {
            let Some(st) = tabs.get(id) else {
                return reply(7);
            };
            if !st.config.meta.options.mark_save_on_close {
                continue;
            }
            let rules = specs(st);
            if st.mark_baseline.as_ref() == Some(&rules) {
                continue;
            }
            if wanted.as_ref().is_some_and(|prior| prior != &rules) {
                return reply(11);
            }
            wanted = Some(rules);
        }
        let Some(wanted) = wanted else {
            return reply(0);
        };
        if super::term_ops::input_locked() {
            return reply(6);
        }
        let disk = match mark_profile::load() {
            Ok(rules) => rules,
            Err(_) => return reply(8),
        };
        if disk != wanted {
            for id in ids {
                let st = &tabs[id];
                let rules = specs(st);
                if st.config.meta.options.mark_save_on_close
                    && st.mark_baseline.as_ref() != Some(&rules)
                    && st.mark_baseline.as_ref() != Some(&disk)
                {
                    return reply(11);
                }
            }
            if mark_profile::save(&wanted).is_err() {
                return reply(8);
            }
        }
        for id in ids {
            let st = tabs.get_mut(id).expect("requested live pane checked above");
            if st.config.meta.options.mark_save_on_close && specs(st) == wanted {
                st.mark_baseline = Some(wanted.clone());
            }
        }
        reply(0)
    })
}
