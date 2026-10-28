//! TCP transport — raw byte-stream socket (no Telnet negotiation).

pub mod config;
pub mod connection;

pub use config::{TcpConfig, TcpConnectionConfig};
pub use connection::TcpInstance;

use crate::error::{Error, Result};
use crate::protocol::{recv_output, uptime_ms, ConnectionInfo, Protocol};
use std::sync::atomic::Ordering;

#[async_trait::async_trait]
impl Protocol for TcpInstance {
    async fn read(&self) -> Result<Vec<u8>> {
        recv_output(&self.output_rx).await
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        use tokio::io::AsyncWriteExt;
        writer
            .write_all(data)
            .await
            .map_err(|e| Error::Tcp(format!("write: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| Error::Tcp(format!("flush: {e}")))?;
        Ok(())
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.cols.store(cols, Ordering::SeqCst);
        self.rows.store(rows, Ordering::SeqCst);
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
        // The output pump can still own an Arc while its pending read retires.
        // Explicitly send FIN instead of waiting for the last writer to drop.
        use tokio::io::AsyncWriteExt;
        self.writer
            .lock()
            .await
            .shutdown()
            .await
            .map_err(|e| Error::Tcp(format!("disconnect: {e}")))
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
    fn tcp_config_reexport() {
        let c = TcpConfig {
            host: "r".into(),
            port: 42,
            terminal_type: "x".into(),
            connect_timeout_secs: 30,
            data_mode: "binary".into(),
        };
        assert_eq!(c.host, "r");
        assert_eq!(c.port, 42);
    }
}
