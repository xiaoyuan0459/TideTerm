//! Serial port connection — async wrapper around `serialport` crate.

use super::config::SerialConfig;
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Live serial port connection with background read task.
pub struct SerialInstance {
    pub(crate) port: Arc<std::sync::Mutex<Box<dyn serialport::SerialPort>>>,
    pub(crate) output_rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
    pub(crate) alive: Arc<AtomicBool>,
    pub(crate) connected_at: Mutex<Instant>,
    pub(crate) read_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl SerialInstance {
    pub async fn connect(config: SerialConfig) -> Result<Self> {
        config.validate()?;
        let path = config.path.clone();
        let baud_rate = config.baud_rate;

        let builder = serialport::new(&path, baud_rate);
        let builder = match config.data_bits {
            5 => builder.data_bits(serialport::DataBits::Five),
            6 => builder.data_bits(serialport::DataBits::Six),
            7 => builder.data_bits(serialport::DataBits::Seven),
            _ => builder.data_bits(serialport::DataBits::Eight),
        };
        let builder = match config.stop_bits {
            2 => builder.stop_bits(serialport::StopBits::Two),
            _ => builder.stop_bits(serialport::StopBits::One),
        };
        let builder = match config.parity.as_str() {
            "odd" => builder.parity(serialport::Parity::Odd),
            "even" => builder.parity(serialport::Parity::Even),
            _ => builder.parity(serialport::Parity::None),
        };
        let builder = match config.flow_control.as_str() {
            "hardware" | "rtscts" => builder.flow_control(serialport::FlowControl::Hardware),
            "xonxoff" | "software" => builder.flow_control(serialport::FlowControl::Software),
            _ => builder.flow_control(serialport::FlowControl::None),
        };

        let port = tokio::task::spawn_blocking(move || {
            builder
                .open()
                .map_err(|e| Error::Serial(format!("open {path}: {e}")))
        })
        .await
        .map_err(|e| Error::Serial(format!("spawn_blocking: {e}")))??;

        let port: Arc<std::sync::Mutex<Box<dyn serialport::SerialPort>>> =
            Arc::new(std::sync::Mutex::new(port));
        let read_port = port.clone();
        let (tx, rx) = mpsc::channel(256);
        // P2-4: shared liveness flag — read_task flips it on any exit path
        // (EOF/error/send failure) so `is_alive()` reflects reality.
        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = alive.clone();

        let read_task = tokio::spawn(async move {
            loop {
                let pc = read_port.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut buf = [0u8; 4096];
                    let mut p = pc.lock().map_err(|_| "mutex poisoned".to_string())?;
                    match p.read(&mut buf) {
                        Ok(0) => Err("EOF".to_string()),
                        Ok(n) => Ok(buf[..n].to_vec()),
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(Vec::new()),
                        Err(e) => Err(format!("{e}")),
                    }
                })
                .await;
                match result {
                    Ok(Ok(data)) => {
                        if !data.is_empty() && tx.send(data).await.is_err() {
                            break;
                        }
                    }
                    Ok(Err(_)) | Err(_) => break,
                }
            }
            // P2-4: connection is gone — mark the session dead.
            alive_clone.store(false, Ordering::SeqCst);
        });

        Ok(Self {
            port,
            output_rx: tokio::sync::Mutex::new(rx),
            alive,
            connected_at: Mutex::new(Instant::now()),
            read_task: tokio::sync::Mutex::new(Some(read_task)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_fails_nonexistent() {
        let c = SerialConfig {
            path: "COM999".into(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
            flow_control: "none".into(),
        };
        assert!(SerialInstance::connect(c).await.is_err());
    }
}
