//! Same-session input/reading mode. The transport and output pump never change.
use super::term::{try_with_tab, TermState};

pub fn term_reading(idx: u32) -> bool {
    try_with_tab(idx, |st| st.reading).unwrap_or(false)
}

pub fn term_reading_search_ready(idx: u32) -> bool {
    try_with_tab(idx, |st| st.reading && st.pane.reading_search_ready()).unwrap_or(false)
}

pub fn term_set_reading(idx: u32, reading: bool) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    try_with_tab(idx, |st| set_reading(st, reading)).is_some()
}

pub(super) fn set_reading(st: &mut TermState, reading: bool) {
    if st.reading == reading {
        return;
    }
    st.reading = reading;
    st.input_revision = st.input_revision.wrapping_add(1);
    st.send_plan = None;
    st.context.shell.clear();
    st.completion_sources.cancel();
    st.preview_revision = st.preview_revision.wrapping_add(1);
    st.pane.set_reading_mode(reading);
    st.cursor.reset();
    st.redraw_pending = true;
}

/// Returns true when a key belongs to mode control or local browsing.
pub(super) fn local_key(st: &mut TermState, key: u32, mods: u32, text: &str) -> bool {
    if key == 14 && mods & 7 == 4 {
        set_reading(st, !st.reading);
        return true;
    }
    if !st.reading {
        return false;
    }
    use tiderm_terminal::pane::reading::ReadAction;
    st.preview_revision = st.preview_revision.wrapping_add(1);
    match st.pane.reading_key(key, mods, text) {
        ReadAction::Exit => set_reading(st, false),
        ReadAction::Paste { bytes, stay } if st.host.alive() && !st.host.modem.busy() => {
            if !stay {
                set_reading(st, false);
            }
            super::command_history::submitted(st, &bytes);
            st.host.write(&bytes);
        }
        _ => {}
    }
    st.redraw_pending = true;
    true
}

/// Explicit paste is the one input operation that exits reading mode.
/// Bytes are already prepared by the clipboard path, including bracketed paste.
pub fn term_paste(idx: u32, bytes: &[u8]) -> bool {
    if bytes.is_empty() || super::term_ops::input_locked() {
        return false;
    }
    let ready = try_with_tab(idx, |st| !st.host.modem.busy() && st.host.alive()).unwrap_or(false);
    if !ready {
        return false;
    }
    term_set_reading(idx, false);
    super::term_input::term_write(idx, bytes);
    true
}
