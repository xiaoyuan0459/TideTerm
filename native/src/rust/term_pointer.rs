//! Pointer routing: reading gestures stay local even in a mouse-grabbing TUI.
use super::term::{scale, with_tab};

pub fn term_wheel(idx: u32, lines: i32, x: f32, y: f32) -> Vec<u8> {
    with_tab(idx, |st| {
        if st.reading || super::term_ops::input_locked() {
            if st.reading {
                st.pane.reading_scroll(lines);
            } else {
                st.pane.scroll_by(lines);
            }
            st.redraw_pending = true;
            return Vec::new();
        }
        let (w, h) = st.viewport_px;
        let s = scale();
        let bytes = st
            .pane
            .wheel(lines, x * s, y * s, w * s, h * s)
            .unwrap_or_default();
        if !bytes.is_empty() {
            st.host.write(&bytes);
        }
        st.redraw_pending = true;
        bytes
    })
}

pub fn term_mouse_send(idx: u32, kind: u8, button: u8, x: f32, y: f32) -> bool {
    with_tab(idx, |st| {
        if st.reading || super::term_ops::input_locked() {
            return false;
        }
        let (w, h) = st.viewport_px;
        let s = scale();
        let Some(bytes) =
            st.pane
                .mouse_report(kind as i32, button as i32, x * s, y * s, w * s, h * s)
        else {
            return false;
        };
        st.host.write(&bytes);
        true
    })
}

pub fn term_mouse(idx: u32, kind: u8, button: u8, _mods: u32, x: f32, y: f32) -> Vec<u8> {
    with_tab(idx, |st| {
        if st.reading || super::term_ops::input_locked() {
            return Vec::new();
        }
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane
            .mouse_report(kind as i32, button as i32, x * s, y * s, w * s, h * s)
            .unwrap_or_default()
    })
}
