//! SSH transport — remote shell via libssh2 (`ssh2` crate).
//!
//! Module structure:
//! - `config` — SshConfig, SshAuth types
//! - `handler` — host-key type/fingerprint helpers, challenge inference
//! - `auth` — Authentication (password, public-key, keyboard-interactive)
//! - `connection` — SshInstance struct, handshake, PTY allocation
//! - `reconnect` — Reconnect logic, background read thread, Drop impl
//! - `sftp` — LibSshSftp (SftpTransport implementation)
//!
//! Public API: `SshConfig`, `SshAuth`, `SshInstance`.

pub mod auth;
pub mod config;
pub mod connection;
mod forward_relay;
mod forwarding;
#[cfg(all(test, windows))]
mod forwarding_tests;
pub mod handler;
pub mod inspect;
mod monitor;
mod pool;
mod raw_channel;
pub mod reconnect;
pub mod sftp;
mod transport;
mod x11;
#[cfg(test)]
mod x11_tests;

#[cfg(test)]
mod env_tests;

use crate::error::Result;
use crate::protocol::{recv_output, uptime_ms, Protocol};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub use config::{SshAuth, SshConfig};
pub use connection::SshInstance;
pub use sftp::LibSshSftp;

#[async_trait::async_trait]
impl Protocol for SshInstance {
    async fn read(&self) -> Result<Vec<u8>> {
        recv_output(&self.output_rx).await
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        let channel = self.channel.lock().await.clone();
        let data = data.to_vec();
        let n = data.len();
        tokio::task::spawn_blocking(move || {
            crate::protocol::ssh::connection::write_channel(&channel, &data)
        })
        .await
        .map_err(|e| crate::error::Error::Ssh(format!("write task panicked: {e}")))??;
        // R-16：静默链路看门狗的武装信号 —— "敲了键却没有任何回字节"才判死，
        // 空闲（无键入）会话永不受影响。必须是**写成功之后**才计数，否则写失败
        // 也会把看门狗武装起来。
        self.bytes_written.fetch_add(n, Ordering::SeqCst);
        Ok(())
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let channel = self.channel.lock().await.clone();
        let session = self.session.lock().await.clone();
        let gate = self.blocking_mode_gate.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut channel = channel;
            let _gate = gate.lock().map_err(|_| {
                crate::error::Error::Ssh("SSH session blocking-mode lock poisoned".into())
            })?;
            // R-21 后交互会话是非阻塞的；request_pty_size 没有自带 EAGAIN
            // 重试，临时切回阻塞（行为与修复前一致：每次调用 2 s 有界）。
            let _blocking = connection::BlockingGuard::new(&session);
            channel
                .request_pty_size(cols as u32, rows as u32, None, None)
                .map_err(|e| crate::error::Error::Ssh(format!("resize: {e}")))
        })
        .await
        .map_err(|e| crate::error::Error::Ssh(format!("resize task panicked: {e}")))??;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn exit_code(&self) -> Option<u32> {
        self.exit_code.lock().ok().and_then(|ec| *ec)
    }

    fn respond_keyboard_interactive(&self, responses: Vec<String>) -> Result<()> {
        let tx = self
            .keyboard_interactive_tx
            .lock()
            .map_err(|_| crate::error::Error::Other("keyboard-interactive lock poisoned".into()))?
            .take()
            .ok_or_else(|| {
                crate::error::Error::Ssh(
                    "no active keyboard-interactive prompt for this session".into(),
                )
            })?;

        tx.send(responses).map_err(|e| {
            crate::error::Error::Ssh(format!("failed to send keyboard-interactive response: {e}"))
        })
    }

    async fn reconnect(&self) -> Result<()> {
        self.reconnect_internal().await
    }

    async fn disconnect(&self) -> Result<()> {
        self.alive.store(false, Ordering::SeqCst);
        {
            let channel = self.channel.lock().await.clone();
            let session = self.session.lock().await.clone();
            let gate = self.blocking_mode_gate.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let Ok(_gate) = gate.lock() else { return };
                let mut channel = channel;
                // R-21 后会话非阻塞；close 需要 EAGAIN 重试，临时切回阻塞
                // 发完 CHANNEL_CLOSE（尽力而为，2 s 有界）。
                let _blocking = connection::BlockingGuard::new(&session);
                let _ = channel.close();
            })
            .await;
        }
        {
            if let Some(active) = self.read_active.lock().await.take() {
                active.store(false, Ordering::SeqCst);
            }
            if let Some(task) = self.read_task.lock().await.take() {
                drop(task);
            }
        }
        Ok(())
    }

    fn connection_info(&self) -> Option<crate::protocol::ConnectionInfo> {
        Some(crate::protocol::ConnectionInfo {
            uptime_ms: uptime_ms(&self.connected_at),
            cipher: None,
            key_exchange: None,
            mac: None,
        })
    }

    async fn open_sftp(&self) -> crate::error::Result<Arc<dyn crate::sftp::SftpTransport>> {
        self.open_sftp_channel().await
    }

    async fn open_sftp_workers(
        &self,
        max_workers: usize,
    ) -> crate::error::Result<Vec<Arc<dyn crate::sftp::SftpTransport>>> {
        self.open_transfer_workers(max_workers).await
    }

    async fn open_system_monitor(&self) -> Result<Arc<dyn crate::system_monitor::SystemMonitor>> {
        self.create_system_monitor().await
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ssh::connection::SshInstance;

    #[tokio::test]
    async fn connect_returns_err_for_unreachable_host() {
        let config = SshConfig {
            host: "127.0.0.1".into(),
            port: 1,
            username: "user".into(),
            auth: SshAuth::Password {
                password: "pass".into(),
            },
            connect_timeout_secs: 5,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            // 与 core 默认一致（120 s）：静默链路看门狗
            link_watchdog_secs: 120,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let inquirer =
            crate::auth::EventHostKeyInquirer::new(crate::event::BackendEventSink::new());
        let kb_backend =
            std::sync::Arc::new(crate::auth::NoopKbBackend) as Arc<dyn crate::auth::KbSyncBackend>;
        let result =
            SshInstance::connect(config, 80, 24, "sess-1".into(), &inquirer, kb_backend).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn connect_returns_err_for_invalid_host() {
        let config = SshConfig {
            host: "this-host-definitely-does-not-exist.invalid".into(),
            port: 22,
            username: "user".into(),
            auth: SshAuth::Password {
                password: "pass".into(),
            },
            connect_timeout_secs: 5,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            // 与 core 默认一致（120 s）：静默链路看门狗
            link_watchdog_secs: 120,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let inquirer =
            crate::auth::EventHostKeyInquirer::new(crate::event::BackendEventSink::new());
        let kb_backend =
            std::sync::Arc::new(crate::auth::NoopKbBackend) as Arc<dyn crate::auth::KbSyncBackend>;
        let result =
            SshInstance::connect(config, 80, 24, "sess-2".into(), &inquirer, kb_backend).await;
        assert!(result.is_err());
    }
}

mod negotiation;
mod proxy;
pub use negotiation::{algorithms, validate_options};
