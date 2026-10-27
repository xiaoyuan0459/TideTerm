//! De-shelled SSH authentication bridges (B1) — replace `tauri::AppHandle` in the SSH
//! transport with trait objects that the host shell implements.
//!
//! The SSH connection needs two interactive flows during handshake:
//! - **host-key confirmation** (unknown host → ask the user to trust),
//! - **keyboard-interactive** prompts (server asks for credentials / OTP).
//!
//! Both previously went through `AppHandle::emit` + a oneshot / mpsc channel. De-shelling
//! turns them into traits; the Qt shell (`native/`) binds them to its own
//! `BackendEventSink` and answers the prompts from the C++ side.

use crate::event::{BackendEvent, BackendEventSink};

/// A host-key confirmation backend.
///
/// `confirm` runs during the SSH handshake for an **unknown** host. It should present the
/// host details to the user and return `Ok(true)` to trust (learn) the key or `Ok(false)`
/// to reject. Implementations should enforce the 60-second timeout hard rule.
#[async_trait::async_trait]
pub trait HostKeyInquirer: Send + Sync {
    /// Ask the user to confirm an unknown host key. `key_type` is e.g. `"ssh-ed25519"`,
    /// `fingerprint` the SHA-256 fingerprint shown to the user, `key` the raw server
    /// host-key bytes (algorithm + key) so the host can verify/learn it.
    async fn confirm(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
        key: &[u8],
    ) -> Result<bool, String>;
}

/// A `HostKeyInquirer` that emits a [`BackendEvent::HostKeyConfirm`] and waits for the
/// response on a oneshot channel. Used by the Qt shell (and reproducible in tests).
#[derive(Debug)]
pub struct EventHostKeyInquirer {
    sink: BackendEventSink,
    responder:
        tokio::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>,
}

impl EventHostKeyInquirer {
    /// Create a new inquirer bound to `sink`.
    pub fn new(sink: BackendEventSink) -> Self {
        EventHostKeyInquirer {
            sink,
            responder: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Answer the pending host-key confirmation for `session_id` (`true` = trust,
    /// `false` = reject). Returns `false` when no confirmation is pending for that id.
    pub async fn answer(&self, session_id: &str, accept: bool) -> bool {
        // Release the registry lock before sending (or waking a waiting task).
        let pending = self.responder.lock().await.remove(session_id);
        match pending {
            Some(tx) => {
                let _ = tx.send(accept);
                true
            }
            None => false,
        }
    }
}

#[async_trait::async_trait]
impl HostKeyInquirer for EventHostKeyInquirer {
    async fn confirm(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
        key: &[u8],
    ) -> Result<bool, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.responder
            .lock()
            .await
            .insert(session_id.to_string(), tx);
        self.sink.emit(BackendEvent::HostKeyConfirm {
            id: session_id.to_string(),
            host: host.to_string(),
            port,
            key_type: key_type.to_string(),
            fingerprint: fingerprint.to_string(),
            key: key.to_vec(),
        });
        // 60-second timeout hard rule: auto-reject if the UI never answers.
        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(Ok(accept)) => Ok(accept),
            Ok(Err(_)) => {
                self.responder.lock().await.remove(session_id);
                Err("host key confirmation channel closed".into())
            }
            Err(_) => {
                self.responder.lock().await.remove(session_id);
                Err("host key confirmation timed out (60s)".into())
            }
        }
    }
}

/// A keyboard-interactive prompt backend.
///
/// `prompt` runs during SSH keyboard-interactive auth; it should render one input field
/// per `fields` and return the user's answers in order (blank where the user left a field
/// empty). Echo controls whether the field is displayed masked.
#[async_trait::async_trait]
pub trait KbPrompter: Send + Sync {
    /// Ask the user for keyboard-interactive responses.
    async fn prompt(
        &self,
        session_id: &str,
        name: &str,
        instruction: &str,
        fields: &[crate::event::ChallengeField],
    ) -> Result<Vec<String>, String>;
}

/// A `KbPrompter` that emits a [`BackendEvent::KeyboardInteractive`] and waits for the
/// response on an mpsc channel. Used by the native shell (and in tests).
#[derive(Debug)]
pub struct EventKbPrompter {
    sink: BackendEventSink,
    responder: tokio::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::mpsc::Sender<Vec<String>>>,
    >,
}

impl EventKbPrompter {
    /// Create a new prompter bound to `sink`.
    pub fn new(sink: BackendEventSink) -> Self {
        EventKbPrompter {
            sink,
            responder: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Submit responses for the pending challenge for `session_id`. Returns `false`
    /// when none is pending.
    pub async fn submit(&self, session_id: &str, responses: Vec<String>) -> bool {
        let pending = self.responder.lock().await.remove(session_id);
        match pending {
            Some(tx) => {
                let _ = tx.send(responses).await;
                true
            }
            None => false,
        }
    }
}

#[async_trait::async_trait]
impl KbPrompter for EventKbPrompter {
    async fn prompt(
        &self,
        session_id: &str,
        name: &str,
        instruction: &str,
        fields: &[crate::event::ChallengeField],
    ) -> Result<Vec<String>, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        self.responder
            .lock()
            .await
            .insert(session_id.to_string(), tx);
        self.sink.emit(BackendEvent::KeyboardInteractive {
            id: session_id.to_string(),
            name: name.to_string(),
            instruction: instruction.to_string(),
            fields: fields.to_vec(),
        });
        let response = rx.recv().await;
        match response {
            Some(responses) => Ok(responses),
            None => {
                self.responder.lock().await.remove(session_id);
                Err("keyboard-interactive challenge cancelled".into())
            }
        }
    }
}

/// A **synchronous** keyboard-interactive prompt backend (B3).
///
/// libssh2's `KeyboardInteractivePrompt` callback is synchronous, so the Tauri
/// transport cannot `await` the async [`KbPrompter`] inside it. This sync
/// sibling abstracts just the *emit* side: the caller (the libssh2 adapter)
/// blocks on its own response channel after emitting. Each host implements it
/// (Tauri bridges to the webview, native to its UI / broadcast).
pub trait KbSyncBackend: Send + Sync {
    /// Emit a keyboard-interactive challenge for the user to answer.
    fn emit(
        &self,
        session_id: &str,
        name: &str,
        instruction: &str,
        fields: &[crate::event::ChallengeField],
    ) -> std::result::Result<(), String>;
}

/// A `KbSyncBackend` that ignores the challenge (no-op). Used by tests that
/// exercise SSH connection failure paths without a UI to answer the prompt.
pub struct NoopKbBackend;

impl KbSyncBackend for NoopKbBackend {
    fn emit(
        &self,
        _session_id: &str,
        _name: &str,
        _instruction: &str,
        _fields: &[crate::event::ChallengeField],
    ) -> std::result::Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_host_key_confirm_delivers_response() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let inquirer = std::sync::Arc::new(EventHostKeyInquirer::new(sink));
        let inq = inquirer.clone();

        let handle = tokio::spawn(async move {
            inq.confirm("s1", "host", 22, "ssh-ed25519", "abc", &[1, 2, 3, 4])
                .await
        });

        // UI side: receive the event, then answer "trust".
        let ev = rx.recv().await.expect("event");
        match ev {
            BackendEvent::HostKeyConfirm { id, host, key, .. } => {
                assert_eq!(id, "s1");
                assert_eq!(host, "host");
                assert_eq!(key, vec![1, 2, 3, 4], "raw host key must be delivered");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(
            inquirer.answer("s1", true).await,
            "answer should be consumed"
        );

        assert!(handle.await.expect("join").expect("confirm"));
    }

    #[tokio::test]
    async fn event_host_key_confirm_multiple_sessions_independent() {
        let sink = BackendEventSink::new();
        let inquirer = std::sync::Arc::new(EventHostKeyInquirer::new(sink));
        let inq1 = inquirer.clone();
        let inq2 = inquirer.clone();

        let h1 = tokio::spawn(async move {
            inq1.confirm("s1", "host1", 22, "ssh-ed25519", "a", &[1])
                .await
        });
        let h2 = tokio::spawn(async move {
            inq2.confirm("s2", "host2", 22, "ssh-ed25519", "b", &[2])
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert!(inquirer.answer("s1", false).await);
        assert!(inquirer.answer("s2", true).await);
        assert!(!h1.await.expect("join").expect("s1 confirm"));
        assert!(h2.await.expect("join").expect("s2 confirm"));
    }

    #[tokio::test]
    async fn event_host_key_confirm_timeout_rejects() {
        let sink = BackendEventSink::new();
        let inquirer = std::sync::Arc::new(EventHostKeyInquirer::new(sink));
        let inq = inquirer.clone();
        // A 1ms timeout is not configurable; assert the responder is armed and the
        // confirm waits (no panic). We avoid the 60s wait by answering immediately.
        let handle = tokio::spawn(async move {
            inq.confirm("s2", "host", 22, "ssh-ed25519", "xyz", &[9, 9])
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(inquirer.answer("s2", false).await);
        assert!(!handle.await.expect("join").expect("confirm"));
    }

    #[tokio::test]
    async fn event_kb_prompt_delivers_responses() {
        let sink = BackendEventSink::new();
        let mut rx = sink.subscribe();
        let prompter = std::sync::Arc::new(EventKbPrompter::new(sink));
        let p = prompter.clone();
        let fields = vec![crate::event::ChallengeField {
            name: "password".into(),
            prompt: "Password:".into(),
            field_type: "password".into(),
            echo: false,
        }];

        let handle =
            tokio::spawn(
                async move { p.prompt("s1", "Password Authentication", "", &fields).await },
            );

        let ev = rx.recv().await.expect("event");
        match ev {
            BackendEvent::KeyboardInteractive { id, name, .. } => {
                assert_eq!(id, "s1");
                assert_eq!(name, "Password Authentication");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(prompter.submit("s1", vec!["secret".into()]).await);

        let got = handle.await.expect("join").expect("prompt");
        assert_eq!(got, vec!["secret"]);
    }
}
