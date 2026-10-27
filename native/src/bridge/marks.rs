pub use crate::rust::mark_persistence::{term_mark_prepare_close, term_mark_set_save_on_close};
pub use crate::rust::term_marks::*;
#[cxx::bridge]
pub mod marks_ffi {
    pub struct MarkRow {
        pub id: u64,
        pub pattern: String,
        pub flags: u32,
        pub color: u8,
        pub enabled: bool,
    }
    pub struct MarkResult {
        pub id: u64,
        pub error: u8,
    }
    pub struct MarkSeed {
        pub text: String,
        pub error: u8,
    }
    pub struct MarkRect {
        pub x: f32,
        pub y: f32,
        pub w: f32,
        pub h: f32,
        pub color: u8,
    }
    extern "Rust" {
        fn term_mark_rows(tab: u32) -> Vec<MarkRow>;
        fn term_mark_seed(tab: u32, selection_only: bool) -> MarkSeed;
        fn term_mark_put(
            tab: u32,
            id: u64,
            pattern: &str,
            flags: u32,
            color: u8,
            enabled: bool,
        ) -> MarkResult;
        fn term_mark_remove(tab: u32, id: u64) -> MarkResult;
        fn term_mark_clear(tab: u32) -> MarkResult;
        fn term_mark_move(tab: u32, id: u64, up: bool) -> MarkResult;
        fn term_mark_quick(tab: u32, color: u8, remove: bool) -> MarkResult;
        fn term_mark_step(tab: u32, id: u64, forward: bool) -> MarkResult;
        fn term_find_selection(tab: u32, forward: bool) -> MarkResult;
        fn term_mark_defaults(tab: u32, save: bool) -> MarkResult;
        fn term_mark_set_save_on_close(tab: u32, enabled: bool) -> MarkResult;
        fn term_mark_prepare_close(ids: &[u32]) -> MarkResult;
        fn term_mark_rects(tab: u32) -> Vec<MarkRect>;
    }
}
