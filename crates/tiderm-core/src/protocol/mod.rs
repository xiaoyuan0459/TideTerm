//! Protocol trait — abstraction layer for terminal transports (B-series).
//!
//! Migrated from the Tauri app crate (protocol/mod.rs) into `tiderm-core` so the
//! native shell can drive the same async transports the app uses, without any Tauri
//! dependency. Every transport (PTY, SSH, Telnet, Serial) implements this trait.
//!
//! The transport-neutral `SftpTransport` trait lives at [`crate::sftp`], which the
//! Tauri app and native shell already share.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod pty_impl;
pub mod serial;
pub mod ssh;
pub mod tcp;
pub mod telnet;

pub use serial::{SerialConfig, SerialInstance};
pub use tcp::{TcpConfig, TcpInstance};
pub use telnet::{TelnetConfig, TelnetInstance};

/// Connection metadata reported in status events.
///
/// Not all transports can expose cipher details; fields are optional
/// and may be omitted when unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Time since the transport became connected, in milliseconds.
    pub uptime_ms: u64,
    /// Symmetric cipher name, if known.
    pub cipher: Option<String>,
    /// Key exchange algorithm name, if known.
    pub key_exchange: Option<String>,
    /// MAC algorithm name, if known.
    pub mac: Option<String>,
}

/// Asynchronous terminal transport abstraction.
///
/// All methods take `&self` — implementations must use interior
/// mutability (e.g. `Mutex`, `AtomicBool`) to satisfy this contract.
#[async_trait::async_trait]
pub trait Protocol: Send + Sync {
    /// Read a chunk of output data from the transport.
    ///
    /// Returns an empty `Vec` when the transport has reached EOF.
    async fn read(&self) -> Result<Vec<u8>>;

    /// Write user input data to the transport.
    async fn write(&self, data: &[u8]) -> Result<()>;

    /// Resize the terminal window (columns × rows).
    async fn resize(&self, cols: u16, rows: u16) -> Result<()>;

    /// Whether the transport is still alive (connected / process running).
    fn is_alive(&self) -> bool;

    /// The process exit code, if available.
    ///
    /// Returns `None` while the process is still running or if the
    /// exit status could not be determined.
    fn exit_code(&self) -> Option<u32>;

    /// Cached directory of this transport's local shell, when supported.
    /// Implementations must keep filesystem IO off the caller's UI thread.
    /// Remote paths and disconnected processes must not appear here.
    fn local_working_directory(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Provide responses for a keyboard-interactive authentication prompt.
    ///
    /// Default implementation returns an error for transports that do not
    /// support keyboard-interactive authentication (e.g. local PTY).
    fn respond_keyboard_interactive(&self, _responses: Vec<String>) -> Result<()> {
        Err(crate::error::Error::Ssh(
            "keyboard-interactive not supported by this transport".into(),
        ))
    }

    /// Attempt to re-establish a dropped transport connection.
    ///
    /// Default implementation returns an error for transports that do not
    /// support automatic reconnection (e.g. local PTY).
    async fn reconnect(&self) -> Result<()> {
        Err(crate::error::Error::Ssh(
            "reconnect not supported by this transport".into(),
        ))
    }

    /// Disconnect the transport without destroying the session entry.
    ///
    /// After disconnect, `is_alive()` returns `false`. The session can
    /// be re-established via `reconnect()`.
    ///
    /// Default implementation returns an error for transports that do
    /// not support explicit disconnect (e.g. local PTY).
    async fn disconnect(&self) -> Result<()> {
        Err(crate::error::Error::Ssh(
            "disconnect not supported by this transport".into(),
        ))
    }

    /// Return connection metadata for status display.
    ///
    /// Default implementation returns `None` for transports that do not
    /// expose connection details (e.g. local PTY).
    fn connection_info(&self) -> Option<ConnectionInfo> {
        None
    }

    /// Open an SFTP subsystem on this connection (SSH only).
    ///
    /// Default implementation returns an error for non-SSH protocols.
    async fn open_sftp(&self) -> Result<Arc<dyn crate::sftp::SftpTransport>> {
        Err(crate::error::Error::Ssh(
            "SFTP not supported for this protocol".into(),
        ))
    }

    /// Separate read-only monitoring channel; unsupported protocols report an error.
    async fn open_system_monitor(&self) -> Result<Arc<dyn crate::system_monitor::SystemMonitor>> {
        Err(crate::error::Error::Other(
            "system monitoring is unavailable for this protocol".into(),
        ))
    }

    /// Open independent transfer connections after the primary SFTP channel is
    /// already available for directory operations.
    async fn open_sftp_workers(
        &self,
        _max_workers: usize,
    ) -> Result<Vec<Arc<dyn crate::sftp::SftpTransport>>> {
        Ok(Vec::new())
    }
}

/// Receive the next output chunk from a transport's mpsc receiver.
///
/// Returns an empty `Vec` on channel close (EOF), matching the
/// `Protocol::read` contract.
pub(crate) async fn recv_output(
    rx: &tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>,
) -> Result<Vec<u8>> {
    let data = rx.lock().await.recv().await;
    Ok(data.unwrap_or_default())
}

/// Compute connection uptime in milliseconds from a `connected_at` timestamp.
///
/// Returns 0 when the lock is poisoned or the timestamp is unavailable.
pub(crate) fn uptime_ms(connected_at: &std::sync::Mutex<std::time::Instant>) -> u64 {
    connected_at
        .lock()
        .ok()
        .map(|at| at.elapsed().as_millis() as u64)
        .unwrap_or(0)
}
