//! De-shelled SFTP transport contract + streaming transfers (B4).
//!
//! The `SftpTransport` trait and the SFTP data types (file entries, stats,
//! transfer progress) live here so both the Tauri app and the native shell
//! share one definition. The streaming `upload_stream` / `download_stream`
//! functions are transport-layer logic decoupled from Tauri: they run purely
//! against a `&dyn SftpTransport` and report progress through a [`ProgressSink`]
//! trait (the Tauri app bridges it to `AppHandle::emit`, the native shell to
//! its UI / broadcast).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

pub use crate::sftp_listing::{DirectoryQuery, DirectoryReadError, DirectorySnapshot};
pub use crate::sftp_types::*;

/// SFTP transport trait.
///
/// Implemented by an SSH session's SFTP subsystem (e.g. libssh2). The
/// de-shelled streaming functions operate on `&dyn SftpTransport`, so callers
/// do not depend on the concrete transport.
#[async_trait::async_trait]
pub trait SftpTransport: Send + Sync {
    /// List directory contents at the given remote path.
    async fn list_dir(&self, path: &str) -> std::result::Result<Vec<SftpFileEntry>, String>;

    /// Bounded, cancellable metadata only. Unsupported implementations must not
    /// emulate this by collecting an unbounded list first.
    async fn list_dir_bounded(
        &self,
        _path: &str,
        query: DirectoryQuery,
    ) -> Result<DirectorySnapshot, DirectoryReadError> {
        query.check()?;
        Err(DirectoryReadError::Unsupported)
    }

    /// Create a directory.
    async fn mkdir(&self, path: &str) -> std::result::Result<(), String>;

    /// Remove a file.
    async fn remove_file(&self, path: &str) -> std::result::Result<(), String>;

    /// Remove one empty directory. Recursive planning belongs to the workflow layer.
    async fn remove_dir(&self, path: &str) -> std::result::Result<(), String>;

    /// Rename/move a file or directory.
    async fn rename(&self, from: &str, to: &str) -> std::result::Result<(), String>;
    async fn rename_new(&self, _from: &str, _to: &str) -> Result<(), String> {
        Err("SFTP non-replacing rename is unsupported".into())
    }

    /// Read an entire file into memory (suitable for small files).
    async fn read_file(&self, path: &str) -> std::result::Result<Vec<u8>, String>;

    /// Write data to a file (create or overwrite).
    async fn write_file(&self, path: &str, data: &[u8]) -> std::result::Result<(), String>;

    /// Get file metadata.
    async fn stat(&self, path: &str) -> std::result::Result<SftpFileStat, String>;

    /// Metadata without following a link; required for bounded recursive operations.
    async fn lstat_optional(&self, _path: &str) -> Result<Option<SftpFileStat>, String> {
        Err("SFTP optional lstat is unsupported".into())
    }
    async fn symlink(&self, _target: &str, _link: &str) -> Result<(), String> {
        Err("SFTP symbolic links are unsupported".into())
    }
    /// Open a new staging file exclusively, never truncating an existing entry.
    async fn open_write_new(
        &self,
        _path: &str,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        Err("SFTP exclusive streaming creation is unsupported".into())
    }

    /// Exclusive creation with an explicit POSIX mode; never fall back to a wider default.
    async fn open_write_new_with_mode(
        &self,
        _path: &str,
        _mode: u32,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        Err("SFTP creation with explicit permissions is unsupported".into())
    }

    /// Update only POSIX permission bits, preserving other metadata.
    async fn set_permissions(&self, _path: &str, _mode: u32) -> Result<(), String> {
        Err("SFTP permission updates are unsupported".into())
    }

    /// Set timestamps; unspecified values are preserved by the server adapter.
    async fn set_times(
        &self,
        _path: &str,
        _accessed: Option<u64>,
        _modified: Option<u64>,
    ) -> Result<(), String> {
        Err("SFTP metadata updates are unsupported by this transport".into())
    }

    /// Whether the SFTP channel is still open.
    fn is_alive(&self) -> bool;

    /// Open a remote file for streaming read.
    ///
    /// The caller is responsible for reading in chunks and emitting progress.
    /// The default implementation falls back to `read_file` wrapped in a
    /// `Cursor`, so existing mock implementations keep working.
    async fn open_read_stream(
        &self,
        path: &str,
    ) -> std::result::Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        let data = self.read_file(path).await?;
        Ok(Box::new(std::io::Cursor::new(data)))
    }

    /// Open a read stream at an absolute file offset for a resumed transfer.
    async fn open_read_stream_from(
        &self,
        path: &str,
        offset: u64,
    ) -> std::result::Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        let mut stream = self.open_read_stream(path).await?;
        let mut limited = (&mut stream).take(offset);
        let copied = tokio::io::copy(&mut limited, &mut tokio::io::sink())
            .await
            .map_err(|e| format!("seek read stream: {e}"))?;
        if copied != offset {
            return Err("read stream ended before resume offset".into());
        }
        Ok(stream)
    }

    /// Open a remote file for streaming write.
    ///
    /// The caller is responsible for writing in chunks and emitting progress.
    /// The default implementation returns a `tokio::io::sink()` that discards
    /// all data — suitable only for tests. Production implementations must
    /// override this.
    async fn open_write_stream(
        &self,
        _path: &str,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        Ok(Box::new(tokio::io::sink()))
    }

    /// Reopen an existing staging file at an absolute offset without truncating it.
    async fn open_write_stream_at(
        &self,
        path: &str,
        offset: u64,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        if offset == 0 {
            self.open_write_stream(path).await
        } else {
            Err("resumable SFTP writes are unsupported by this transport".into())
        }
    }
}

/// A destination for SFTP transfer progress events.
///
/// Replaces the Tauri `AppHandle::emit` coupling in the streaming loop; the
/// Tauri app implements it by emitting to the webview, the native shell by
/// pushing to its UI / broadcast.
#[async_trait::async_trait]
pub trait ProgressSink: Send + Sync {
    /// Report a transfer-progress update.
    async fn progress(&self, p: SftpTransferProgress) -> std::result::Result<(), String>;
}

pub use crate::sftp_stream::{download_stream, upload_stream};

#[cfg(test)]
#[path = "sftp_tests.rs"]
mod tests;
