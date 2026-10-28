pub use crate::rust::term_selection::{
    term_column_selection, term_multi_selection, term_path_selection, term_selection_active,
    term_selection_active_rects, term_selection_begin_add, term_selection_begin_column,
    term_selection_cycle, term_selection_extend_to, term_selection_locked, term_selection_preview,
    term_selection_remove_active, term_selection_status, term_selection_swap_anchor,
    term_selection_text, term_selection_token, term_selection_transform, term_set_column_selection,
    term_set_multi_selection, term_set_path_selection, term_set_selection_locked,
};

#[cxx::bridge]
pub mod selection_ffi {
    #[derive(Default)]
    pub struct SelectionText {
        pub text: String,
        pub exceeds_budget: bool,
    }
    #[derive(Default)]
    pub struct SelectionPreview {
        pub available: bool,
        pub text: String,
        pub truncated: bool,
        pub exceeds_budget: bool,
    }
    #[derive(Default)]
    pub struct SelectionStatus {
        pub active_id: u64,
        pub active: u32,
        pub count: u32,
    }
    pub struct SelectionRect {
        pub x: f32,
        pub y: f32,
        pub w: f32,
        pub h: f32,
    }
    extern "Rust" {
        fn term_path_selection(idx: u32) -> bool;
        fn term_set_path_selection(idx: u32, enabled: bool) -> bool;
        fn term_selection_locked(idx: u32) -> bool;
        fn term_set_selection_locked(idx: u32, locked: bool) -> bool;
        fn term_selection_token(idx: u32, x: f32, y: f32) -> bool;
        /// 0 success; 1 no multiline selection; 2 locked; 3 budget; 4 retired; 5 screen locked.
        fn term_selection_transform(idx: u32, invert: bool) -> u8;
        fn term_selection_active_rects(idx: u32) -> Vec<SelectionRect>;
        fn term_multi_selection(idx: u32) -> bool;
        fn term_set_multi_selection(idx: u32, enabled: bool) -> bool;
        fn term_selection_begin_add(idx: u32, x: f32, y: f32, column: bool) -> bool;
        fn term_selection_cycle(idx: u32, forward: bool) -> bool;
        fn term_selection_remove_active(idx: u32) -> bool;
        fn term_selection_status(idx: u32) -> SelectionStatus;
        fn term_column_selection(idx: u32) -> bool;
        fn term_selection_text(idx: u32) -> SelectionText;
        fn term_selection_preview(idx: u32) -> SelectionPreview;
        fn term_set_column_selection(idx: u32, column: bool) -> bool;
        fn term_selection_begin_column(idx: u32, x: f32, y: f32);
        fn term_selection_extend_to(idx: u32, x: f32, y: f32, column: bool) -> bool;
        fn term_selection_swap_anchor(idx: u32) -> bool;
        fn term_selection_active(idx: u32) -> bool;
    }
    unsafe extern "C++" {
        include!("cpp/bridge_shim.h");
        fn qt_boot_selection(png_out: &str) -> i32;
    }
}
