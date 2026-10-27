pub use crate::rust::sync_channels::{
    term_sync_close, term_sync_create, term_sync_replace, term_sync_snapshot, term_sync_status,
    term_sync_update,
};

#[cxx::bridge]
pub mod sync_ffi {
    struct SyncChannel {
        name: String,
        members: Vec<u32>,
    }
    #[derive(Default)]
    struct SyncStatus {
        channel: u8,
        name: String,
        members: u32,
    }
    extern "Rust" {
        fn term_sync_create() -> u64;
        fn term_sync_close(owner: u64);
        fn term_sync_update(owner: u64, views: &[u32]) -> bool;
        fn term_sync_snapshot(owner: u64) -> Vec<SyncChannel>;
        fn term_sync_replace(owner: u64, channels: &[SyncChannel]) -> u8;
        fn term_sync_status(owner: u64, source: u32) -> SyncStatus;
    }
}
