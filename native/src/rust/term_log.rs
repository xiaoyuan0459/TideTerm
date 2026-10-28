//! Qt 日志面板与会话捕获之间的有界读写接口。

use super::term::with_tab;

pub fn slog_start(idx: u32) {
    with_tab(idx, |st| {
        if let Err(error) = st.log.start_disk() {
            *st.note.borrow_mut() = error;
        }
    });
}

pub fn slog_pause(idx: u32, paused: bool) {
    with_tab(idx, |st| st.log.pause(paused));
}
pub fn slog_pending_drains() -> usize {
    super::continuous_log::pending_drains()
}

pub fn slog_status(idx: u32) -> crate::bridge::log::log_ffi::LogStatus {
    with_tab(idx, |st| {
        let status = st.log.disk_status();
        crate::bridge::log::log_ffi::LogStatus {
            state: status.state,
            path: status.path,
            error: status.error,
            written: status.written,
            retained: status.retained,
        }
    })
}

pub fn slog_stop(idx: u32) {
    with_tab(idx, |st| st.log.set_on(false));
}

pub fn slog_active(idx: u32) -> bool {
    with_tab(idx, |st| st.log.on())
}

pub fn slog_revision(idx: u32) -> u64 {
    with_tab(idx, |st| st.log.revision())
}

pub fn slog_recent(idx: u32, limit: u32) -> String {
    with_tab(idx, |st| st.log.recent(limit as usize))
}

pub fn slog_clear(idx: u32) {
    with_tab(idx, |st| st.log.clear());
}

pub fn slog_export(idx: u32) -> Result<String, String> {
    with_tab(idx, |st| {
        let title = super::term_diag::display_title(st);
        st.log.export(&title).map(|p| p.display().to_string())
    })
}
