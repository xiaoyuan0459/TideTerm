//! Bounded asynchronous local directory snapshots.
pub use crate::rust::file_jobs::{
    files_quote_path, files_task_cancel, files_task_poll, files_task_start,
};
pub use crate::rust::local_files::{files_close, files_create, files_poll, files_request};
pub use crate::rust::remote_edit::files_edit_close;
#[cxx::bridge]
pub mod files_ffi {
    struct FileTask {
        id: u64,
        edit: u64,
        state: u8,
        done: u64,
        total: u64,
        bytes: u64,
        path: String,
        error: String,
    }
    pub struct LocalEntry {
        pub name: String,
        pub is_dir: bool,
        pub size: u64,
        pub modified: i64,
    }
    pub struct LocalSnapshot {
        pub ready: bool,
        pub path: String,
        pub error: String,
        pub truncated: bool,
        pub entries: Vec<LocalEntry>,
    }
    extern "Rust" {
        fn files_task_start(
            owner: u64,
            op: u8,
            source_uid: &str,
            source: &str,
            target_uid: &str,
            target: &str,
            policy: u8,
        ) -> Result<u64>;
        fn files_task_poll(owner: u64, id: u64) -> FileTask;
        fn files_task_cancel(owner: u64, id: u64);
        fn files_edit_close(owner: u64, id: u64, discard: bool) -> Result<()>;
        fn files_create() -> u64;
        fn files_close(owner: u64);
        fn files_request(owner: u64, path: &str);
        fn files_poll(owner: u64) -> LocalSnapshot;
        fn files_quote_path(path: &str, dialect: u8) -> Result<String>;
    }
}
