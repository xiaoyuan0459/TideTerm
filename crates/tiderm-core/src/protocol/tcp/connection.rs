//! TCP connection — raw byte-stream socket transport.
//!
//! Unlike Telnet, no RFC 854 NVT / IAC negotiation is performed: every
//! inbound byte is forwarded to the terminal and every user keystroke is
//! written verbatim to the socket. This matches the reference UI's
//! "raw socket" semantics. When `data_mode == "text"`, outgoing `\n` is
//! translated to `\r\n` to emulate a line-oriented peer.

use super::config::TcpConfig;
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Live raw TCP connection with a background read task.
pub struct TcpInstance {
    pub(crate) writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    pub(crate) output_rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
    pub(crate) alive: Arc<AtomicBool>,
    pub(crate) cols: AtomicU16,
    pub(crate) rows: AtomicU16,
    pub(crate) connected_at: Mutex<Instant>,
    pub(crate) read_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    #[allow(dead_code)]
    config: TcpConfig,
}

impl TcpInstance {
    pub async fn connect(config: TcpConfig, cols: u16, rows: u16) -> Result<Self> {
        config.validate()?;
        let addr = format!("{}:{}", config.host, config.port);
        let timeout = Duration::from_secs(config.connect_timeout_secs);
        let stream = tokio::time::timeout(timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Tcp(format!("connection timed out: {addr}")))?
            .map_err(|e| Error::Tcp(format!("connect {addr}: {e}")))?;
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
        let (tx, rx) = mpsc::channel(256);
        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = alive.clone();
        // read_task flips `alive` on EOF/error so `is_alive()` reflects reality.
        let read_task = tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                if tx.send(buf[..n].to_vec()).await.is_err() {
                    break;
                }
            }
            alive_clone.store(false, Ordering::SeqCst);
        });
        Ok(Self {
            writer,
            output_rx: tokio::sync::Mutex::new(rx),
            alive,
            cols: AtomicU16::new(cols),
            rows: AtomicU16::new(rows),
            connected_at: Mutex::new(Instant::now()),
            read_task: tokio::sync::Mutex::new(Some(read_task)),
            config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;

    #[tokio::test]
    async fn connect_fails_unreachable() {
        let cfg = TcpConfig {
            host: "127.0.0.1".into(),
            port: 1,
            terminal_type: "xterm".into(),
            connect_timeout_secs: 2,
            data_mode: "binary".into(),
        };
        assert!(TcpInstance::connect(cfg, 80, 24).await.is_err());
    }

    /// When the peer closes the connection, the read task must flip
    /// `alive` to false so `is_alive()` reflects reality.
    #[tokio::test]
    async fn read_task_marks_dead_after_peer_closes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let listener = Arc::new(listener);
        let addr = listener.local_addr().expect("local addr");

        // Accept then immediately drop the stream: the client reads EOF.
        let acceptor = listener.clone();
        let server = tokio::spawn(async move {
            let (_stream, _) = acceptor.accept().await.expect("accept");
        });

        let cfg = TcpConfig {
            host: "127.0.0.1".into(),
            port: addr.port(),
            terminal_type: "xterm".into(),
            connect_timeout_secs: 5,
            data_mode: "binary".into(),
        };
        let inst = TcpInstance::connect(cfg, 80, 24).await.expect("connect");
        server.await.expect("server task");

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !inst.is_alive() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("alive should flip to false after peer closes");

        // A closed tab may leave its protocol Arc in an output pump briefly.
        // The peer must observe EOF even while this instance remains alive.
        let connected = TcpInstance::connect(inst.config.clone(), 80, 24)
            .await
            .expect("reconnect");
        let (mut peer, _) = listener.accept().await.expect("accept reconnect");
        connected.disconnect().await.expect("disconnect");
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), peer.read(&mut byte))
                .await
                .expect("disconnect must close the wire before the instance drops")
                .expect("read EOF"),
            0
        );
    }
}
