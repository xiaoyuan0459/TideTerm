pub use crate::rust::workspace::*;

#[cxx::bridge]
pub mod workspace_ffi {
    unsafe extern "C++" {
        include!("cpp/bridge_shim.h");
        fn qt_boot_workspace(png: &str) -> i32;
    }
    extern "Rust" {
        fn workspace_capture(layout: &str, ids: &[u32]) -> Result<String>;
        fn workspace_save(key: &str, data: &str) -> Result<()>;
        fn workspace_load(key: &str) -> Result<String>;
        fn workspace_list() -> Vec<String>;
        fn workspace_layout(data: &str) -> Result<String>;
        fn workspace_secret_flags(data: &str, index: usize) -> Result<u8>;
        fn workspace_open(
            data: &str,
            index: usize,
            password: &str,
            passphrase: &str,
            proxy: &str,
        ) -> Result<u32>;
    }
}
