//! De-shelled backend event channel (B1/B8) — replaces `tauri::Emitter` + `AppHandle`
//! with a `tokio::broadcast`-backed sink that both the Tauri app and the native shell
//! can subscribe to. The transport read-loops emit [`BackendEvent`] instead of calling
//! `app_handle.emit(...)`.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// A backend event that the read-loops / transports produce.
///
/// This is the transport-neutral superset of the old `event.rs` payloads. The Tauri app
/// maps these back to `OutputEvent`/`ExitEvent`/… JSON for the webview; the native shell
/// maps them into its own `SessionEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackendEvent {
    /// Raw output bytes for a session (never lossy-converted; the receiver decodes by charset).
    Output { id: String, data: Vec<u8> },
    /// A session exited.
    Exit { id: String, code: Option<u32> },
    /// A session errored.
    Error { id: String, message: String },
    /// Connection status changed (`connecting`/`connected`/`disconnected`/`reconnecting`/`error`).
    Status {
        id: String,
        status: String,
        message: Option<String>,
    },
    /// SSH: the server's host key needs confirmation. `key` is the raw server host-key
    /// bytes (algorithm + key), so the receiving host can verify/learn it without
    /// re-fetching from the transport.
    HostKeyConfirm {
        id: String,
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
        key: Vec<u8>,
    },
    /// SSH: a keyboard-interactive challenge was issued.
    KeyboardInteractive {
        id: String,
        name: String,
        instruction: String,
        fields: Vec<ChallengeField>,
    },
    /// The sync-input group changed.
    SyncInputState { ids: Vec<String> },
}

/// A single keyboard-interactive challenge field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeField {
    pub name: String,
    pub prompt: String,
    pub field_type: String,
    pub echo: bool,
}

/// A cloneable handle for emitting [`BackendEvent`]s to all subscribers.
///
/// Mirrors the old `AppHandle::emit` surface with a plain method, so transport code can
/// hold a `BackendEventSink` and `emit(...)` without any Tauri dependency.
#[derive(Debug, Clone)]
pub struct BackendEventSink {
    tx: broadcast::Sender<BackendEvent>,
}

impl Default for BackendEventSink {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendEventSink {
    /// Bounded backlog for the broadcast channel. A slow subscriber (the Qt shell's
    /// 16ms render pump) lags instead of blocking the transport read-loop, but a too-small
    /// capacity drops bursts of terminal output (e.g. `cat` of a large file). 4096 keeps
    /// normal bursts lossless; raise further only if `dropped_events` still climbs under
    /// extreme output (see the shell's `drain_events`).
    ///
    /// Public so the shell's headless test can flood exactly this many events and assert
    /// on the real capacity instead of a copied magic number.
    pub const EVENT_CHANNEL_CAPACITY: usize = 4096;

    /// Create a new sink with a fresh broadcast channel.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(Self::EVENT_CHANNEL_CAPACITY);
        BackendEventSink { tx }
    }

    /// Emit an event to all subscribers (best-effort; a dropped subscriber is ignored).
    pub fn emit(&self, event: BackendEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to events from this sink.
    pub fn subscribe(&self) -> broadcast::Receiver<BackendEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_emits_to_subscriber() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        sink.emit(BackendEvent::Output {
            id: "pane-0".into(),
            data: b"hi".to_vec(),
        });
        let got = rx.try_recv().expect("received");
        match got {
            BackendEvent::Output { id, data } => {
                assert_eq!(id, "pane-0");
                assert_eq!(data, b"hi");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn sink_multiple_subscribers_all_receive() {
        let sink = BackendEventSink::new();
        let mut rx1 = sink.subscribe();
        let mut rx2 = sink.subscribe();
        sink.emit(BackendEvent::Exit {
            id: "s".into(),
            code: Some(0),
        });
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    /// R-11 的**可判定内核**：超出容量后，订阅者要么收到、要么被明确告知漏了多少条，
    /// 不允许事件凭空消失。若本测例变红，说明连 core 这一层都在静默丢，
    /// 壳层 `drain_events` 拿到 `Lagged` 也无法补救。
    #[test]
    fn backlog_beyond_capacity_is_reported_not_silent() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let cap = BackendEventSink::EVENT_CHANNEL_CAPACITY;
        let total = cap + 1000;
        for i in 0..total {
            sink.emit(BackendEvent::Output {
                id: "p".into(),
                data: vec![i as u8],
            });
        }
        let mut got = 0usize;
        let mut lagged = 0u64;
        loop {
            match rx.try_recv() {
                Ok(_) => got += 1,
                Err(broadcast::error::TryRecvError::Lagged(n)) => lagged += n,
                _ => break,
            }
        }
        println!(
            "[R-11] 容量={cap} 发 {total} 条 → 实收 {got} + Lagged 上报 {lagged} = {}",
            got as u64 + lagged
        );
        assert_eq!(
            got as u64 + lagged,
            total as u64,
            "既没送达也没被告知 —— 事件在 core 层就凭空消失"
        );
    }

    /// 事件总线**没有补发**：`emit` 在无订阅者时把 `send` 的 `Err` 直接丢弃（`let _ =`），
    /// 之后 `subscribe` 只能看到其后的事件。壳层若在 core 开始产出之后才订阅，
    /// 早期的 `Status`/`Error` 就是看不见的一次性事件。
    #[test]
    fn emit_before_first_subscribe_is_discarded() {
        let sink = BackendEventSink::new();
        sink.emit(BackendEvent::Error {
            id: "early".into(),
            message: "boom".into(),
        });
        let mut rx = sink.subscribe();
        let got = rx.try_recv();
        println!("[无补发] 订阅前 emit 1 条 → 订阅后 try_recv = {got:?}");
        assert!(
            got.is_err(),
            "意外收到了订阅前的事件 —— 说明存在补发机制，判据需相应改写"
        );
    }

    #[test]
    fn challenge_field_serdes_round_trip() {
        let f = ChallengeField {
            name: "password".into(),
            prompt: "Password:".into(),
            field_type: "password".into(),
            echo: false,
        };
        let json = serde_json::to_string(&f).expect("serialize");
        let dec: ChallengeField = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(dec.prompt, "Password:");
        assert!(!dec.echo);
    }
}
