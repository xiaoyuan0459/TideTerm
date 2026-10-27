//! SFTP data types shared by the core SFTP transport contract and the UI layers.
//!
//! Kept separate from `sftp.rs` so the streaming implementation stays under its
//! tech-debt freeze line.

use serde::{Deserialize, Serialize};

/// File entry from `list_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_unix: i64,
    pub permissions: u32,
    /// Owner name from the SFTP server's longname extension.
    /// `None` when the server does not return user metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Group name from the SFTP server's longname extension.
    /// `None` when the server does not return group metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// File metadata from `stat`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpFileStat {
    #[serde(default)]
    pub accessed_unix: Option<i64>,
    pub size_bytes: u64,
    pub modified_unix: i64,
    pub permissions: u32,
    pub is_dir: bool,
    /// Owner name (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Group name (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// Transfer progress event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpTransferProgress {
    pub session_id: String,
    pub file_name: String,
    pub direction: TransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Upload,
    Download,
}
