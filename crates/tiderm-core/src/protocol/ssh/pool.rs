//! Independent SSH sessions for parallel small-file SFTP transfers.

use super::config::{SshAuth, SshConfig};
use super::connection::{connect_session, run_bounded, SshInstance};
use super::sftp::LibSshSftp;
use crate::error::{Error, Result};
use crate::sftp::SftpTransport;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const MAX_WORKERS: usize = 32;

impl SshInstance {
    pub(super) async fn open_transfer_workers(
        &self,
        max_workers: usize,
    ) -> Result<Vec<Arc<dyn SftpTransport>>> {
        let mut channels = Vec::new();
        if max_workers == 0 || matches!(self.config.auth, SshAuth::KeyboardInteractive) {
            return Ok(channels);
        }
        let trusted_key = self
            .session
            .lock()
            .await
            .host_key()
            .ok_or_else(|| Error::Ssh("primary SSH session has no host key".into()))?
            .0
            .to_vec();
        let mut pending = tokio::task::JoinSet::new();
        for _ in 0..max_workers.min(MAX_WORKERS) {
            let config = self.config.clone();
            let trusted_key = trusted_key.clone();
            let alive = self.alive.clone();
            pending.spawn(async move { open_extra(config, trusted_key, alive).await });
        }
        while let Some(result) = pending.join_next().await {
            match result {
                Ok(Ok(channel)) if self.alive.load(Ordering::SeqCst) => channels.push(channel),
                Ok(Ok(_)) => break,
                Ok(Err(error)) => {
                    tracing::warn!(target: "ssh", "extra SFTP session unavailable: {error}")
                }
                Err(error) => tracing::warn!(target: "ssh", "extra SFTP task failed: {error}"),
            }
        }
        Ok(channels)
    }
}

async fn open_extra(
    config: SshConfig,
    trusted_key: Vec<u8>,
    alive: Arc<AtomicBool>,
) -> Result<Arc<dyn SftpTransport>> {
    let (session, cancel) = authenticated_extra(&config, &trusted_key).await?;
    let init_session = session.clone();
    let raw = run_bounded(
        &cancel,
        Duration::from_secs(config.connect_timeout_secs),
        "extra SFTP init",
        move || {
            init_session
                .sftp()
                .map_err(|error| Error::Ssh(format!("extra SFTP init: {error}")))
        },
    )
    .await?;
    session.set_timeout(30_000);
    session.set_blocking(false);
    Ok(Arc::new(
        LibSshSftp::new(session, raw, alive, Arc::new(Mutex::new(())))
            .with_creation_modes(config.options.sftp_file_mode, config.options.sftp_dir_mode),
    ))
}

/// Shared extra-connection security boundary: pin the already verified terminal key.
pub(super) async fn authenticated_extra(
    config: &SshConfig,
    trusted_key: &[u8],
) -> Result<(ssh2::Session, std::net::TcpStream)> {
    static HANDSHAKES: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    let _permit = HANDSHAKES
        .get_or_init(|| tokio::sync::Semaphore::new(4))
        .acquire()
        .await
        .map_err(|_| Error::Ssh("SFTP handshake limiter closed".into()))?;
    let (session, cancel) = connect_session(config).await?;
    cancel
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| Error::Ssh(format!("extra SFTP read timeout: {error}")))?;
    cancel
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| Error::Ssh(format!("extra SFTP write timeout: {error}")))?;
    let presented_key = session
        .host_key()
        .ok_or_else(|| Error::Ssh("extra SFTP session has no host key".into()))?
        .0;
    if presented_key != trusted_key {
        return Err(Error::Ssh(
            "extra SFTP host key differs from terminal session".into(),
        ));
    }
    let auth_session = session.clone();
    let auth_config = config.clone();
    run_bounded(
        &cancel,
        Duration::from_secs(config.connect_timeout_secs),
        "extra SFTP auth",
        move || super::auth::authenticate(&auth_session, &auth_config),
    )
    .await?;
    Ok((session, cancel))
}
