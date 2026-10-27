pub use crate::rust::palette::*;
#[cxx::bridge]
pub mod palette_ffi {
    pub struct LineState {
        pub context: u64,
        pub current: u64,
        pub total: u64,
        pub error: u8,
    }
    pub struct LineTarget {
        pub token: u64,
        pub line: u64,
        pub text: String,
        pub error: u8,
    }
    pub struct PaletteSession {
        pub tab: u32,
        pub saved: String,
        pub title: String,
        pub protocol: String,
        pub group: String,
    }
    pub struct PaletteSessions {
        pub rows: Vec<PaletteSession>,
        pub truncated: bool,
        pub error: u8,
    }
    extern "Rust" {
        fn palette_allowed() -> bool;
        fn term_line_begin(tab: u32, generation: u32) -> LineState;
        fn term_line_preview(tab: u32, generation: u32, context: u64, query: &str) -> LineTarget;
        fn term_line_commit(tab: u32, generation: u32, context: u64, token: u64) -> bool;
        fn term_line_end(tab: u32, context: u64);
        fn palette_sessions(tabs: &[u32]) -> PaletteSessions;
        fn palette_saved_exists(id: &str) -> bool;
    }
}
