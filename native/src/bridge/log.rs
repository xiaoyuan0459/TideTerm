//! log 域桥（ADR-014 按域拆桥第七域，Phase C ⑧）。
//!
//! 会话日志开关/导出/行数（⑨ LogView 读面后续扩）。`slog_` 前缀跨桥全局
//! 唯一（`log_` 与系统宏太近，留安全边距）；只传 POD。

pub use crate::rust::log_settings::{slog_default_settings, slog_save_settings, slog_settings};
pub use crate::rust::term_log::{
    slog_active, slog_clear, slog_export, slog_pause, slog_pending_drains, slog_recent,
    slog_revision, slog_start, slog_status, slog_stop,
};

#[cxx::bridge]
pub mod log_ffi {
    struct LogSettings {
        directory: String,
        prefix: String,
        format: u8,
        rotate_mib: u32,
        keep: u32,
    }
    struct LogStatus {
        state: u8,
        path: String,
        error: String,
        written: u64,
        retained: u64,
    }
    extern "Rust" {
        fn slog_settings() -> Result<LogSettings>;
        fn slog_default_settings() -> Result<LogSettings>;
        fn slog_save_settings(options: LogSettings) -> Result<()>;
        fn slog_pause(idx: u32, paused: bool);
        fn slog_status(idx: u32) -> LogStatus;
        fn slog_pending_drains() -> usize;
        /// 开启活动窗格会话日志捕获。
        fn slog_start(idx: u32);
        /// 停止捕获（环保留——stop 后仍可导出已捕获内容）。
        fn slog_stop(idx: u32);
        /// 捕获是否开启。
        fn slog_active(idx: u32) -> bool;
        /// 最新日志版本；视图只在版本变化时取有界快照。
        fn slog_revision(idx: u32) -> u64;
        fn slog_recent(idx: u32, limit: u32) -> String;
        fn slog_clear(idx: u32);
        /// 导出独立日志快照，返回绝对路径；空内容和写盘失败返回可见错误。
        fn slog_export(idx: u32) -> Result<String>;
    }
}
