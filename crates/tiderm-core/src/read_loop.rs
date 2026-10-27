//! De-shelled output read-loop (B1) — the `emit → tokio::broadcast` replacement of the
//! Tauri-backed `read_loop.rs`. Reads from any [`ReadSource`] and forwards micro-batched
//! output into a [`BackendEventSink`], with EOF/error + reconnect handling.

use crate::event::{BackendEvent, BackendEventSink};
use tokio::sync::Mutex;

/// Micro-batch flush threshold in bytes.
const BATCH_SIZE_BYTES: usize = 8 * 1024;
/// Micro-batch flush time window in milliseconds.
const BATCH_WINDOW_MS: u64 = 5;
/// Upper bound on a single emit payload.
const MAX_BATCH_PER_EMIT: usize = 64 * 1024;

/// A transport-neutral async byte source (the `Protocol::read` surface, de-shelled).
///
/// Implementations return `Ok(vec![])` on EOF and `Err(_)` on a transport error. The
/// native shell and the Tauri backend both adapt their concrete transports to this.
#[async_trait::async_trait]
pub trait ReadSource: Send + Sync {
    /// Read the next chunk of output (empty = EOF).
    async fn read(&self) -> Result<Vec<u8>, String>;
    /// Whether the source is still alive.
    fn is_alive(&self) -> bool;
}

/// Spawn the status ticker + output pump for a session, emitting via `sink`.
pub fn spawn_read_loop(id: String, source: std::sync::Arc<dyn ReadSource>, sink: BackendEventSink) {
    spawn_status_ticker(id.clone(), source.clone(), sink.clone());
    tokio::spawn(output_pump(id, source, sink));
}

/// Emit a `Status` heartbeat every 5s while the source is alive.
fn spawn_status_ticker(id: String, source: std::sync::Arc<dyn ReadSource>, sink: BackendEventSink) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if !source.is_alive() {
                break;
            }
            sink.emit(BackendEvent::Status {
                id: id.clone(),
                status: "connected".into(),
                message: None,
            });
        }
    });
}

/// Continuously read from the source and forward micro-batched output to the sink.
///
/// On EOF or error the pending buffer is flushed first, then a Status + Exit/Error event
/// is emitted and the loop terminates (reconnect is delegated to the owning session).
async fn output_pump(id: String, source: std::sync::Arc<dyn ReadSource>, sink: BackendEventSink) {
    let mut buf: Vec<u8> = Vec::with_capacity(MAX_BATCH_PER_EMIT);
    let mut last_flush = std::time::Instant::now();

    loop {
        // Keep one read alive across flushes: cancelling a blocking PTY read
        // would discard its eventual bytes. Quiet prompts still need a deadline.
        let read = source.read();
        tokio::pin!(read);
        let result = loop {
            if buf.is_empty() {
                break (&mut read).await;
            }
            let deadline = tokio::time::Instant::from_std(
                last_flush + std::time::Duration::from_millis(BATCH_WINDOW_MS),
            );
            tokio::select! {
                result = &mut read => break result,
                _ = tokio::time::sleep_until(deadline) => {
                    flush_buffer(&id, &mut buf, &sink);
                    last_flush = std::time::Instant::now();
                }
            }
        };
        match result {
            Ok(data) => {
                if data.is_empty() {
                    flush_buffer(&id, &mut buf, &sink);
                    sink.emit(BackendEvent::Status {
                        id: id.clone(),
                        status: "disconnected".into(),
                        message: None,
                    });
                    sink.emit(BackendEvent::Exit {
                        id: id.clone(),
                        code: None,
                    });
                    break;
                }
                buf.extend_from_slice(&data);
                let should_flush = buf.len() >= BATCH_SIZE_BYTES
                    || buf.len() >= MAX_BATCH_PER_EMIT
                    || last_flush.elapsed().as_millis() as u64 >= BATCH_WINDOW_MS;
                if should_flush {
                    flush_buffer(&id, &mut buf, &sink);
                    last_flush = std::time::Instant::now();
                }
            }
            Err(e) => {
                flush_buffer(&id, &mut buf, &sink);
                sink.emit(BackendEvent::Status {
                    id: id.clone(),
                    status: "error".into(),
                    message: Some(e.clone()),
                });
                sink.emit(BackendEvent::Error {
                    id: id.clone(),
                    message: e,
                });
                break;
            }
        }
    }
}

/// Flush the micro-batch buffer as a single `Output` event, retaining capacity.
fn flush_buffer(id: &str, buf: &mut Vec<u8>, sink: &BackendEventSink) {
    if buf.is_empty() {
        return;
    }
    let data = buf.clone();
    buf.clear();
    sink.emit(BackendEvent::Output {
        id: id.into(),
        data,
    });
}

/// A `ReadSource` backed by an mpsc receiver — feeds a runloop without a real transport.
pub struct ChannelReader {
    rx: Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    alive: std::sync::atomic::AtomicBool,
}

impl ChannelReader {
    /// Create a reader from a live mpsc receiver.
    pub fn new(rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        ChannelReader {
            rx: Mutex::new(rx),
            alive: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

#[async_trait::async_trait]
impl ReadSource for ChannelReader {
    async fn read(&self) -> Result<Vec<u8>, String> {
        let data = self.rx.lock().await.recv().await;
        match data {
            Some(data) => Ok(data),
            None => {
                self.alive.store(false, std::sync::atomic::Ordering::SeqCst);
                Ok(vec![])
            }
        }
    }

    fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pump_forwards_output_and_exit() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let (tx, chan_rx) = tokio::sync::mpsc::channel(16);
        let reader = std::sync::Arc::new(ChannelReader::new(chan_rx));
        spawn_read_loop("pane-0".into(), reader, sink);

        tx.send(b"hello".to_vec()).await.expect("send");
        // Give the pump time to flush the batch window (5ms) before EOF.
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
        drop(tx);
        // Let the pump observe EOF and emit the deferred flush + exit.
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

        let mut saw_output = false;
        let mut saw_exit = false;
        for _ in 0..8 {
            match rx.try_recv() {
                Ok(BackendEvent::Output { data, .. }) if data == b"hello" => saw_output = true,
                Ok(BackendEvent::Exit { .. }) => saw_exit = true,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_output, "expected output event");
        assert!(saw_exit, "expected exit event");
    }

    #[tokio::test]
    async fn pump_flushes_short_output_without_more_data_or_eof() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let (tx, chan_rx) = tokio::sync::mpsc::channel(16);
        let reader = std::sync::Arc::new(ChannelReader::new(chan_rx));
        spawn_read_loop("quiet-pane".into(), reader, sink);
        let query = b"\x1b[6n";
        tx.send(query.to_vec()).await.expect("send cursor query");

        let output = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            loop {
                if let BackendEvent::Output { data, .. } = rx.recv().await.expect("event bus") {
                    break data;
                }
            }
        })
        .await;
        assert_eq!(
            output.expect("short output must flush while the connection stays open"),
            query
        );
        drop(tx);
    }

    #[tokio::test]
    async fn pump_preserves_non_utf8_bytes() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let (tx, chan_rx) = tokio::sync::mpsc::channel(16);
        let reader = std::sync::Arc::new(ChannelReader::new(chan_rx));
        spawn_read_loop("pane-bytes".into(), reader, sink);

        let raw: Vec<u8> = vec![0xFF, 0xFE, b'h', b'i'];
        tx.send(raw.clone()).await.expect("send");
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
        drop(tx);
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

        let mut saw_raw = false;
        for _ in 0..8 {
            match rx.try_recv() {
                Ok(BackendEvent::Output { data, .. }) if data == raw => saw_raw = true,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_raw, "expected raw non-UTF-8 output event");
    }

    #[test]
    fn batch_constants() {
        assert_eq!(BATCH_SIZE_BYTES, 8 * 1024);
        assert_eq!(BATCH_WINDOW_MS, 5);
        assert_eq!(MAX_BATCH_PER_EMIT, 64 * 1024);
    }
}
