//! Saved session catalog. Live connections belong to the resource explorer.
use crate::bridge::session::session_ffi::SessRow;

pub fn sess_explorer_rows() -> Vec<SessRow> {
    // The catalog already carries user-chosen groups. A synthetic Shell parent
    // makes every saved SSH/Telnet/etc. connection look like a local shell.
    super::store::tree_rows()
}
