//! Linux monitoring uses a separate authenticated channel and never writes into the PTY.
use crate::error::{Error, Result};
use crate::system_monitor::{linux_sample, SystemMonitor, SystemSample};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct LinuxMonitor {
    session: ssh2::Session,
    socket: std::net::TcpStream,
    previous: Mutex<Option<(u64, u64)>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for LinuxMonitor {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

impl super::SshInstance {
    pub(super) async fn create_system_monitor(&self) -> Result<Arc<dyn SystemMonitor>> {
        if matches!(self.config.auth, super::SshAuth::KeyboardInteractive) {
            return Err(Error::Ssh(
                "system monitoring requires password or key authentication".into(),
            ));
        }
        let key = self
            .session
            .lock()
            .await
            .host_key()
            .ok_or_else(|| Error::Ssh("primary SSH session has no host key".into()))?
            .0
            .to_vec();
        let mut config = self.config.clone();
        config.connect_timeout_secs = config.connect_timeout_secs.clamp(1, 5);
        let (session, socket) = super::pool::authenticated_extra(&config, &key).await?;
        session.set_timeout(3000);
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| Error::Ssh(format!("monitor timeout: {e}")))?;
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| Error::Ssh(format!("monitor timeout: {e}")))?;
        Ok(Arc::new(LinuxMonitor {
            session,
            socket,
            previous: Mutex::new(None),
            alive: self.alive.clone(),
        }))
    }
}

#[async_trait::async_trait]
impl SystemMonitor for LinuxMonitor {
    async fn sample(&self) -> Result<SystemSample> {
        if !self.alive.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::Ssh("SSH session disconnected".into()));
        }
        let session = self.session.clone();
        let text = super::connection::run_bounded(
            &self.socket,
            Duration::from_secs(3),
            "system sample",
            move || {
                let mut channel = session
                    .channel_session()
                    .map_err(|e| Error::Ssh(format!("system channel: {e}")))?;
                channel
                    .exec("cat /proc/stat /proc/meminfo")
                    .map_err(|e| Error::Ssh(format!("system command: {e}")))?;
                let mut text = String::new();
                channel
                    .by_ref()
                    .take(65537)
                    .read_to_string(&mut text)
                    .map_err(|e| Error::Ssh(format!("system read: {e}")))?;
                if text.len() > 65536 {
                    return Err(Error::Ssh("system reply exceeds 64 KiB".into()));
                }
                let _ = channel.close();
                Ok(text)
            },
        )
        .await?;
        let mut previous = self
            .previous
            .lock()
            .map_err(|_| Error::Other("monitor lock poisoned".into()))?;
        linux_sample(&text, &mut previous)
    }
}
