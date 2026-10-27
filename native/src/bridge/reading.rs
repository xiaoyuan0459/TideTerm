pub use crate::rust::history_export::{
    history_export_close, history_export_page, history_export_pager, history_export_poll,
    history_export_start,
};
pub use crate::rust::term_history::{
    term_history_preview, term_history_scroll_to, term_history_state,
};
pub use crate::rust::term_reading::{
    term_paste, term_reading, term_reading_search_ready, term_set_reading,
};
pub use crate::rust::tmux_session::{tmux_cancel, tmux_prepare, tmux_send};

#[cxx::bridge]
pub mod reading_ffi {
    #[derive(Default)]
    pub struct ExportState {
        // 0 absent, 1 exporting, 2 ready, 3 external pager, 4 done, 5 error, 6 cancelled.
        pub state: u8,
        pub completed: u32,
        pub rows: u32,
        pub bytes: u64,
        pub mapped: bool,
        pub error: String,
    }
    #[derive(Default)]
    pub struct TmuxPreview {
        pub token: u64,
        pub command: String,
        pub error: String,
    }
    #[derive(Default)]
    pub struct HistoryState {
        pub available: bool,
        pub position: u32,
        pub maximum: u32,
        pub rows: u32,
        pub cols: u32,
        pub generation: u32,
        pub revision: u64,
    }
    #[derive(Default)]
    pub struct HistoryText {
        pub available: bool,
        pub first_line: u64,
        pub text: String,
        pub truncated: bool,
    }
    extern "Rust" {
        fn history_export_start(source: u32, target: &str, limit_mb: u32, map: bool) -> String;
        fn history_export_poll(source: u32) -> ExportState;
        fn history_export_page(source: u32, page: u32) -> HistoryText;
        fn history_export_close(source: u32);
        fn history_export_pager(source: u32, executable: &str, kind: u8) -> String;
        fn tmux_prepare(source: u32, name: &str, action: u8) -> TmuxPreview;
        fn tmux_send(source: u32, token: u64, posix_empty_confirmed: bool) -> String;
        fn tmux_cancel(source: u32);
        fn term_history_state(idx: u32) -> HistoryState;
        fn term_history_preview(idx: u32, position: u32) -> HistoryText;
        fn term_history_scroll_to(idx: u32, position: u32) -> bool;
        fn term_reading(idx: u32) -> bool;
        fn term_reading_search_ready(idx: u32) -> bool;
        fn term_set_reading(idx: u32, reading: bool) -> bool;
        fn term_paste(idx: u32, bytes: &[u8]) -> bool;
    }
    unsafe extern "C++" {
        include!("cpp/bridge_shim.h");
        fn qt_boot_reading(png_out: &str) -> i32;
        fn qt_boot_history(png_out: &str) -> i32;
    }
}
