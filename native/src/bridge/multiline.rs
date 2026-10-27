pub use crate::rust::multiline::*;
#[cxx::bridge]
pub mod multiline_ffi {
    pub struct SendOptions {
        pub eol: u8,
        pub tabs: u8,
        pub append: bool,
        pub paced: bool,
    }
    pub struct SendPreview {
        pub text: String,
        pub bytes: u32,
        pub lines: u32,
        pub error: u8,
    }
    pub struct SendProgress {
        pub sent: u32,
        pub total: u32,
        pub error: u8,
    }
    extern "Rust" {
        fn multiline_begin(source: u32, targets: &[u32], paste: u8) -> u64;
        fn multiline_preview(text: &str, options: &SendOptions) -> SendPreview;
        fn multiline_prepare(
            source: u32,
            token: u64,
            text: &str,
            options: &SendOptions,
        ) -> SendProgress;
        fn multiline_step(source: u32, token: u64) -> SendProgress;
        fn multiline_end(source: u32, token: u64);
        fn multiline_valid(source: u32, token: u64) -> bool;
    }
}
