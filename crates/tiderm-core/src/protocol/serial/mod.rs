//! Serial port transport — local serial device via `serialport` crate.

pub mod config;
pub mod connection;

pub use config::{SerialConfig, SerialConnectionConfig, SerialPortInfo};
pub use connection::SerialInstance;

use crate::error::{Error, Result};
use crate::protocol::{recv_output, uptime_ms, ConnectionInfo, Protocol};
use std::sync::atomic::Ordering;

/// Enumerate available serial ports on the system.
pub fn enumerate_ports() -> Result<Vec<SerialPortInfo>> {
    let ports = serialport::available_ports()
        .map_err(|e| Error::Serial(format!("enumerate ports: {e}")))?;
    Ok(ports
        .into_iter()
        .map(|p| {
            let name = p.port_name.clone();
            SerialPortInfo {
                path: p.port_name,
                name: Some(name),
                port_type: match &p.port_type {
                    serialport::SerialPortType::UsbPort(_) => "usb".into(),
                    serialport::SerialPortType::BluetoothPort => "bluetooth".into(),
                    serialport::SerialPortType::PciPort => "pci".into(),
                    serialport::SerialPortType::Unknown => "unknown".into(),
                },
            }
        })
        .collect())
}

#[async_trait::async_trait]
impl Protocol for SerialInstance {
    async fn read(&self) -> Result<Vec<u8>> {
        recv_output(&self.output_rx).await
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        let port = self.port.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut p = port
                .lock()
                .map_err(|_| Error::Serial("mutex poisoned".into()))?;
            p.write_all(&data)
                .map_err(|e| Error::Serial(format!("write: {e}")))?;
            p.flush().map_err(|e| Error::Serial(format!("flush: {e}")))
        })
        .await
        .map_err(|e| Error::Serial(format!("spawn_blocking: {e}")))?
    }

    async fn resize(&self, _cols: u16, _rows: u16) -> Result<()> {
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
        Ok(())
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
    fn serial_config_reexport() {
        let c = SerialConfig {
            path: "COM1".into(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
            flow_control: "none".into(),
        };
        assert_eq!(c.baud_rate, 9600);
    }
    #[test]
    fn enumerate_no_panic() {
        let _ = enumerate_ports();
    }
}
