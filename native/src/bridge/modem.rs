use crate::rust::modem::{modem_cancel, modem_progress, modem_start};
#[cxx::bridge]
pub mod modem_ffi {
    struct ModemProgress {
        busy: bool,
        finished: bool,
        bytes: u64,
        file: String,
        error: String,
    }
    extern "Rust" {
        fn modem_start(tab: u32, kind: u8, receive: bool, paths: Vec<String>) -> String;
        fn modem_cancel(tab: u32);
        fn modem_progress(tab: u32) -> ModemProgress;
    }
}
