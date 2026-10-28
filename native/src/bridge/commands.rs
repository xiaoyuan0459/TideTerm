pub use crate::rust::term_commands::{
    term_command_at_point, term_command_fold, term_command_fold_current, term_command_jump,
    term_command_output, term_command_select_block, term_command_step, term_command_text,
    term_commands, term_fold_markers,
};
pub use crate::rust::term_folds::{
    term_fold_count, term_fold_jump, term_fold_remove, term_fold_rows, term_fold_selection,
    term_fold_selection_status, term_fold_set,
};

#[cxx::bridge]
pub mod commands_ffi {
    pub struct FoldMarker {
        pub id: u64,
        pub x: f32,
        pub y: f32,
        pub w: f32,
        pub h: f32,
        pub folded: bool,
    }
    pub struct FoldResult {
        pub id: u64,
        pub error: u8,
    }
    pub struct FoldRow {
        pub id: u64,
        pub parent: u64,
        pub depth: u32,
        pub title: String,
        pub first: u64,
        pub last: u64,
        pub folded: bool,
        pub manual: bool,
    }
    pub struct CommandRow {
        pub id: u64,
        pub title: String,
        pub line: u64,
        pub finished: bool,
        pub folded: bool,
        pub foldable: bool,
        pub startup: bool,
    }
    #[derive(Default)]
    pub struct CommandText {
        pub available: bool,
        pub text: String,
        pub truncated: bool,
    }
    extern "Rust" {
        fn term_command_at_point(idx: u32, x: f32, y: f32) -> u64;
        fn term_command_text(idx: u32, id: u64) -> CommandText;
        fn term_command_select_block(idx: u32, id: u64) -> bool;
        fn term_fold_markers(idx: u32) -> Vec<FoldMarker>;
        fn term_fold_rows(idx: u32) -> Vec<FoldRow>;
        fn term_fold_count(idx: u32) -> u32;
        fn term_fold_selection_status(idx: u32) -> u8;
        fn term_fold_selection(idx: u32) -> FoldResult;
        fn term_fold_set(idx: u32, id: u64, folded: bool, recursive: bool) -> bool;
        fn term_fold_remove(idx: u32, id: u64) -> bool;
        fn term_fold_jump(idx: u32, id: u64) -> bool;
        fn term_commands(idx: u32) -> Vec<CommandRow>;
        fn term_command_output(idx: u32, id: u64, preview: bool) -> CommandText;
        fn term_command_jump(idx: u32, id: u64) -> bool;
        fn term_command_step(idx: u32, forward: bool) -> bool;
        fn term_command_fold(idx: u32, id: u64, folded: bool) -> bool;
        fn term_command_fold_current(idx: u32) -> bool;
    }
    unsafe extern "C++" {
        include!("cpp/bridge_shim.h");
        fn qt_boot_commands(png_out: &str) -> i32;
    }
}
