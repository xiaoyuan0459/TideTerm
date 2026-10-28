//! Bounded metadata snapshots for interactive consumers; no unbounded fallback.
use crate::sftp_types::SftpFileEntry;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct DirectoryQuery {
    pub max_entries: usize,
    /// Sum of UTF-8 name and path lengths; entry metadata is bounded by max_entries.
    pub max_name_bytes: usize,
    pub deadline: Instant,
    pub canceled: Arc<AtomicBool>,
}
impl DirectoryQuery {
    pub fn check(&self) -> Result<(), DirectoryReadError> {
        if self.max_entries == 0
            || self.max_entries > 2048
            || self.max_name_bytes == 0
            || self.max_name_bytes > 262144
        {
            return Err(DirectoryReadError::InvalidRequest);
        }
        if self.canceled.load(Ordering::Relaxed) {
            return Err(DirectoryReadError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(DirectoryReadError::TimedOut);
        }
        Ok(())
    }
}
#[derive(Debug, Default)]
pub struct DirectorySnapshot {
    pub entries: Vec<SftpFileEntry>,
    /// Budget reached; may be true when the directory has exactly max_entries items.
    pub truncated: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DirectoryReadError {
    Unsupported,
    InvalidRequest,
    Cancelled,
    TimedOut,
    Io(String),
}
impl std::fmt::Display for DirectoryReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("bounded directory listing is unsupported"),
            Self::InvalidRequest => f.write_str("invalid directory request or budget"),
            Self::Cancelled => f.write_str("directory listing cancelled"),
            Self::TimedOut => f.write_str("directory listing timed out"),
            Self::Io(error) => f.write_str(error),
        }
    }
}
impl std::error::Error for DirectoryReadError {}
