//! Terminal menu operations and focus-aware cursor presentation.
use super::store::session_config::SessionKind;
use super::term::{scale, try_with_tab, with_tab};
use std::time::Instant;

pub struct CursorBlink {
    epoch: Instant,
    focused: bool,
}

impl Default for CursorBlink {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            focused: false,
        }
    }
}

impl CursorBlink {
    pub fn reset(&mut self) {
        self.epoch = Instant::now();
    }

    fn visible(&mut self, focused: bool, enabled: bool) -> bool {
        if self.focused != focused {
            self.focused = focused;
            self.reset();
        }
        !focused || !enabled || (self.epoch.elapsed().as_millis() / 500).is_multiple_of(2)
    }
}

/// Driven by the existing Qt pump; hidden/inactive panes keep a steady cursor.
pub fn term_cursor_tick(idx: u32, focused: bool) {
    let enabled = crate::theme::prefs_load().cursor_blink;
    try_with_tab(idx, |st| {
        let visible = st.cursor.visible(focused, enabled);
        if st.opts.cursor_on != visible {
            st.opts.cursor_on = visible;
            st.redraw_pending = true;
        }
    });
}

/// 0=scrollback, 1=screen, 2=both. Display-only; no escape bytes sent to the peer.
pub fn term_clear(idx: u32, kind: u8) {
    with_tab(idx, |st| {
        st.preview_revision = st.preview_revision.wrapping_add(1);
        match kind {
            0 => st.pane.clear_scrollback(),
            1 => st.pane.clear_screen(),
            2 => st.pane.clear(),
            _ => return,
        }
        st.redraw_pending = true;
    });
}

/// Return a guarded URL; only local sessions may resolve paths on this machine.
pub fn term_link_target(idx: u32, x: f32, y: f32) -> String {
    if !x.is_finite() || !y.is_finite() {
        return String::new();
    }
    with_tab(idx, |st| {
        let local = st.config.kind == SessionKind::Local;
        st.pane.set_cwd(local.then(|| st.context.cwd.clone()));
        let s = scale();
        let (w, h) = st.viewport_px;
        let hit = if local {
            st.pane.link_at(x * s, y * s, w * s, h * s)
        } else {
            st.pane.url_link_at(x * s, y * s, w * s, h * s)
        };
        let Some(hit) = hit else {
            return String::new();
        };
        let target = if hit.is_url {
            url::Url::parse(&hit.abs_path)
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https"))
        } else if local {
            url::Url::from_file_path(&hit.abs_path).ok()
        } else {
            None
        };
        target.map(|u| u.to_string()).unwrap_or_default()
    })
}
