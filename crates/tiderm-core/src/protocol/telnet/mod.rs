//! Telnet transport — remote terminal via RFC 854 NVT.

pub mod config;
pub mod connection;

pub use config::{TelnetConfig, TelnetConnectionConfig};
pub use connection::TelnetInstance;

use crate::error::{Error, Result};
use crate::protocol::{recv_output, uptime_ms, ConnectionInfo, Protocol};
use std::sync::atomic::Ordering;

#[async_trait::async_trait]
impl Protocol for TelnetInstance {
    async fn read(&self) -> Result<Vec<u8>> {
        recv_output(&self.output_rx).await
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        use tokio::io::AsyncWriteExt;
        // IAC byte escaping: double every 0xFF in user input
        let mut escaped = Vec::with_capacity(data.len() + data.len() / 64);
        for &b in data {
            escaped.push(b);
            if b == 0xFF {
                escaped.push(0xFF);
            }
        }
        writer
            .write_all(&escaped)
            .await
            .map_err(|e| Error::Telnet(format!("write: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| Error::Telnet(format!("flush: {e}")))?;
        Ok(())
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.cols.store(cols, Ordering::SeqCst);
        self.rows.store(rows, Ordering::SeqCst);
        // NAWS: IAC SB NAWS ch cl rh rl IAC SE
        let naws = [
            0xFF,
            0xFA,
            0x1F,
            (cols >> 8) as u8,
            (cols & 0xFF) as u8,
            (rows >> 8) as u8,
            (rows & 0xFF) as u8,
            0xFF,
            0xF0,
        ];
        let mut writer = self.writer.lock().await;
        use tokio::io::AsyncWriteExt;
        writer
            .write_all(&naws)
            .await
            .map_err(|e| Error::Telnet(format!("naws: {e}")))?;
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn exit_code(&self) -> Option<u32> {
        None
    }

    async fn disconnect(&self) -> Result<()> {
        self.alive.store(false, Ordering::SeqCst);
        if let Some(task) = self.read_task.lock().await.take() {
            task.abort();
        }
        use tokio::io::AsyncWriteExt;
        self.writer
            .lock()
            .await
            .shutdown()
            .await
            .map_err(|e| Error::Telnet(format!("disconnect: {e}")))
    }

    fn connection_info(&self) -> Option<ConnectionInfo> {
        Some(ConnectionInfo {
            uptime_ms: uptime_ms(&self.connected_at),
            cipher: None,
            key_exchange: None,
            mac: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn telnet_config_reexport() {
        let c = TelnetConfig {
            host: "r".into(),
            port: 23,
            terminal_type: "x".into(),
            connect_timeout_secs: 30,
        };
        assert_eq!(c.host, "r");
    }
}
