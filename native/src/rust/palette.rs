//! Palette business data and native admission. Qt keeps filtering/focus and window ownership.
use super::term::try_with_tab;
use crate::bridge::palette::palette_ffi::{LineState, LineTarget, PaletteSession, PaletteSessions};
use std::collections::HashSet;

pub fn term_line_begin(tab: u32, generation: u32) -> LineState {
    if super::term_ops::input_locked() {
        return LineState {
            context: 0,
            current: 0,
            total: 0,
            error: 5,
        };
    }
    try_with_tab(tab, |st| {
        if st.generation != generation {
            return LineState {
                context: 0,
                current: 0,
                total: 0,
                error: 7,
            };
        }
        match st.pane.line_begin() {
            Ok(c) => LineState {
                context: c.id,
                current: c.current,
                total: c.total,
                error: 0,
            },
            Err(e) => LineState {
                context: 0,
                current: 0,
                total: 0,
                error: e as u8,
            },
        }
    })
    .unwrap_or(LineState {
        context: 0,
        current: 0,
        total: 0,
        error: 6,
    })
}
pub fn term_line_preview(tab: u32, generation: u32, context: u64, query: &str) -> LineTarget {
    let failure = |error| LineTarget {
        token: 0,
        line: 0,
        text: String::new(),
        error,
    };
    if super::term_ops::input_locked() {
        return failure(5);
    }
    try_with_tab(tab, |st| {
        if st.generation != generation {
            return failure(7);
        }
        match st.pane.line_preview(context, query) {
            Ok(t) => LineTarget {
                token: t.token,
                line: t.line,
                text: t.text,
                error: 0,
            },
            Err(e) => failure(e as u8),
        }
    })
    .unwrap_or_else(|| failure(6))
}
pub fn term_line_commit(tab: u32, generation: u32, context: u64, token: u64) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(tab, |st| {
        if st.generation != generation || !st.pane.line_can_commit(context, token) {
            return false;
        }
        super::term_reading::set_reading(st, true);
        let ok = st.pane.line_commit(context, token);
        st.redraw_pending |= ok;
        ok
    })
    .unwrap_or(false)
}
pub fn term_line_end(tab: u32, context: u64) {
    try_with_tab(tab, |st| st.pane.line_end(context));
}

pub fn palette_allowed() -> bool {
    !super::term_ops::input_locked()
}

fn bounded(value: &str) -> String {
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}
fn protocol(kind: super::store::SessionKind) -> String {
    use super::store::SessionKind;
    match kind {
        SessionKind::Local => "Shell",
        SessionKind::Ssh => "SSH",
        SessionKind::Telnet => "Telnet",
        SessionKind::Serial => "Serial",
        SessionKind::Tcp => "TCP",
    }
    .into()
}
pub fn palette_sessions(tabs: &[u32]) -> PaletteSessions {
    if super::term_ops::input_locked() {
        return PaletteSessions {
            rows: Vec::new(),
            truncated: false,
            error: 5,
        };
    }
    let catalog = super::store::list_all_checked();
    let error = if catalog.is_err() { 8 } else { 0 };
    let catalog = catalog.unwrap_or_default();
    let mut rows = Vec::new();
    let mut opened = HashSet::new();
    let mut seen = HashSet::new();
    for &tab in tabs.iter().take(4096) {
        if !seen.insert(tab) {
            continue;
        }
        if let Some((saved, name, kind, group)) = try_with_tab(tab, |st| {
            (
                st.config.id.clone(),
                st.config.display_name(),
                st.config.kind,
                st.config.meta.group_path.clone(),
            )
        }) {
            opened.insert(saved.clone());
            let name = catalog
                .iter()
                .find(|c| c.id == saved)
                .map_or(name, |c| c.display_name());
            let title = super::term_diag::term_title(tab);
            rows.push(PaletteSession {
                tab,
                saved,
                title: bounded(&if title == name {
                    title
                } else {
                    format!("{title} · {name}")
                }),
                protocol: protocol(kind),
                group: bounded(&group),
            });
        }
    }
    let mut truncated = tabs.len() > 4096;
    for cfg in catalog {
        if opened.contains(&cfg.id) {
            continue;
        }
        if rows.len() == 4096 {
            truncated = true;
            break;
        }
        if cfg.id.len() > 512 {
            truncated = true;
            continue;
        }
        rows.push(PaletteSession {
            tab: u32::MAX,
            saved: cfg.id.clone(),
            title: bounded(&cfg.display_name()),
            protocol: protocol(cfg.kind),
            group: bounded(&cfg.meta.group_path),
        });
    }
    PaletteSessions {
        rows,
        truncated,
        error,
    }
}
pub fn palette_saved_exists(id: &str) -> bool {
    !super::term_ops::input_locked() && super::store::get_by_id_checked(id).ok().flatten().is_some()
}
