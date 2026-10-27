//! OneKey account manager bridge. Secrets are input only and never returned.

pub use crate::rust::onekey::{onekey_delete, onekey_rows, onekey_save};

#[cxx::bridge]
pub mod onekey_ffi {
    pub struct OneKeyRow {
        pub id: String,
        pub name: String,
        pub username: String,
        pub kind: u8,
        pub key_path: String,
    }

    extern "Rust" {
        fn onekey_rows() -> Vec<OneKeyRow>;
        fn onekey_save(
            id: &str,
            name: &str,
            username: &str,
            kind: u8,
            secret: &str,
            key_path: &str,
        ) -> String;
        fn onekey_delete(id: &str) -> bool;
    }
}
