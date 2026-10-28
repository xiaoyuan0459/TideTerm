//! Session manager — tracks live terminal sessions (de-shelled).
//!
//! Migrated from the Tauri app crate (`session.rs`) into `tiderm-core` so the
//! native shell and the app share the same session lifecycle. Emitted events are
//! produced by the transport read-loops via [`crate::event::BackendEventSink`].

use crate::auth::{HostKeyInquirer, KbSyncBackend};
use crate::error::Result;
use crate::protocol::pty_impl::PtyInstance;
use crate::protocol::ssh::{SshConfig, SshInstance};
use crate::protocol::Protocol;
use crate::sftp::SftpTransport;
use std::collections::HashMap;
use std::sync::Arc;

/// A live session entry.
pub struct SessionEntry {
    pub protocol: Arc<dyn Protocol>,
    pub kind: String,
    pub title: String,
    /// Optional SFTP channel — present on SSH sessions that have opened
    /// the SFTP subsystem via `sftp_open`.
    pub sftp: Option<Arc<dyn SftpTransport>>,
}

impl SessionEntry {
    /// Forward keyboard-interactive responses to the underlying protocol.
    pub fn respond_keyboard_interactive(&self, responses: Vec<String>) -> Result<()> {
        self.protocol.respond_keyboard_interactive(responses)
    }
}

/// Manages all active terminal sessions.
pub struct SessionManager {
    sessions: HashMap<String, SessionEntry>,
    /// IDs of sessions in the current synchronized input group.
    /// Input sent to any member is broadcast to all members.
    sync_group: Vec<String>,
}

impl SessionManager {
    /// Create an empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            sync_group: Vec::new(),
        }
    }

    /// Create a new local shell session and return its ID.
    ///
    /// If `id` is `Some`, uses the provided ID. Otherwise generates a new UUID.
    pub async fn create_local(
        &mut self,
        id: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<String> {
        let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let pty = PtyInstance::spawn(cols, rows, "auto")?;
        let protocol: Arc<dyn Protocol> = Arc::new(pty);

        self.sessions.insert(
            id.clone(),
            SessionEntry {
                protocol,
                kind: "local".into(),
                title: "Local Shell".into(),
                sftp: None,
            },
        );

        Ok(id)
    }

    /// Create a new SSH session and return its ID.
    ///
    /// `inquirer` resolves unknown-host-key confirmations and `kb_backend`
    /// bridges the synchronous keyboard-interactive prompt (both de-shelled).
    pub async fn create_ssh(
        &mut self,
        id: Option<String>,
        config: SshConfig,
        cols: u16,
        rows: u16,
        inquirer: &dyn HostKeyInquirer,
        kb_backend: Arc<dyn KbSyncBackend>,
    ) -> Result<String> {
        let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let title = ssh_title(&config);
        let ssh =
            SshInstance::connect(config, cols, rows, id.clone(), inquirer, kb_backend).await?;
        let protocol: Arc<dyn Protocol> = Arc::new(ssh);

        self.sessions.insert(
            id.clone(),
            SessionEntry {
                protocol,
                kind: "ssh".into(),
                title,
                sftp: None,
            },
        );

        Ok(id)
    }

    /// Insert a pre-built local protocol under the given ID.
    pub fn insert_local(&mut self, id: String, protocol: Arc<dyn Protocol>) {
        self.sessions.insert(
            id,
            SessionEntry {
                protocol,
                kind: "local".into(),
                title: "Local Shell".into(),
                sftp: None,
            },
        );
    }

    /// Insert a pre-built SSH protocol under the given ID with a title.
    pub fn insert_ssh(&mut self, id: String, protocol: Arc<dyn Protocol>, title: String) {
        self.sessions.insert(
            id,
            SessionEntry {
                protocol,
                kind: "ssh".into(),
                title,
                sftp: None,
            },
        );
    }

    /// Insert a pre-built protocol under the given ID with an explicit kind/title.
    pub fn insert_protocol(
        &mut self,
        id: String,
        protocol: Arc<dyn Protocol>,
        kind: &str,
        title: String,
    ) {
        self.sessions.insert(
            id,
            SessionEntry {
                protocol,
                kind: kind.into(),
                title,
                sftp: None,
            },
        );
    }

    /// Get a session by ID.
    pub fn get(&self, id: &str) -> Option<&SessionEntry> {
        self.sessions.get(id)
    }

    /// Get the protocol handle for a session.
    pub fn get_protocol(&self, id: &str) -> Option<Arc<dyn Protocol>> {
        self.sessions.get(id).map(|e| e.protocol.clone())
    }

    /// Remove and destroy a session.
    pub fn remove(&mut self, id: &str) -> Option<SessionEntry> {
        self.sessions.remove(id)
    }

    /// List all active sessions as (id, kind, title, alive) tuples.
    pub fn list(&self) -> Vec<(String, String, String, bool)> {
        self.sessions
            .iter()
            .map(|(id, entry)| {
                (
                    id.clone(),
                    entry.kind.clone(),
                    entry.title.clone(),
                    entry.protocol.is_alive(),
                )
            })
            .collect()
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether there are no active sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Set the synchronized input group to the given session IDs.
    pub fn set_sync_group(&mut self, ids: Vec<String>) {
        self.sync_group = ids;
    }

    /// Clear the synchronized input group.
    pub fn clear_sync_group(&mut self) {
        self.sync_group.clear();
    }

    /// Return the current sync input group IDs.
    pub fn sync_group(&self) -> &[String] {
        &self.sync_group
    }

    /// Return protocol handles for all sessions in the sync group.
    pub fn sync_group_protocols(&self) -> Vec<Arc<dyn Protocol>> {
        self.sync_group
            .iter()
            .filter_map(|id| self.sessions.get(id))
            .map(|e| e.protocol.clone())
            .collect()
    }

    /// Set the SFTP transport for a session (called after opening the SFTP subsystem).
    pub fn set_sftp(&mut self, id: &str, sftp: Arc<dyn SftpTransport>) -> Result<()> {
        let entry = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| crate::error::Error::Other(format!("session not found: {id}")))?;
        entry.sftp = Some(sftp);
        Ok(())
    }

    /// Get the SFTP transport for a session, if available.
    pub fn get_sftp(&self, id: &str) -> Option<Arc<dyn SftpTransport>> {
        self.sessions.get(id).and_then(|e| e.sftp.clone())
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a human-readable title for an SSH session.
fn ssh_title(config: &SshConfig) -> String {
    format!("{}@{}", config.username, config.host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let manager = SessionManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[tokio::test]
    async fn create_local_returns_ok_with_valid_id() {
        let mut manager = SessionManager::new();
        let result = manager.create_local(None, 80, 24).await;
        assert!(result.is_ok());
        let id = result.unwrap();
        assert!(!id.is_empty());
        assert_eq!(manager.len(), 1);
    }

    #[tokio::test]
    async fn get_returns_entry_after_create() {
        let mut manager = SessionManager::new();
        let id = manager.create_local(None, 80, 24).await.expect("create");
        let entry = manager.get(&id);
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.kind, "local");
    }

    #[tokio::test]
    async fn get_protocol_returns_arc() {
        let mut manager = SessionManager::new();
        let id = manager.create_local(None, 80, 24).await.expect("create");
        let protocol = manager.get_protocol(&id);
        assert!(protocol.is_some());
    }

    #[tokio::test]
    async fn remove_deletes_session() {
        let mut manager = SessionManager::new();
        let id = manager.create_local(None, 80, 24).await.expect("create");
        assert_eq!(manager.len(), 1);
        let removed = manager.remove(&id);
        assert!(removed.is_some());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn ssh_title_formats_correctly() {
        let config = SshConfig {
            options: Default::default(),
            host: "example.com".into(),
            port: 22,
            username: "admin".into(),
            auth: crate::protocol::ssh::SshAuth::Password {
                password: "secret".into(),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            link_watchdog_secs: 120,
            term: "xterm-256color".into(),
        };
        assert_eq!(ssh_title(&config), "admin@example.com");
    }

    #[test]
    fn sync_group_starts_empty() {
        let manager = SessionManager::new();
        assert!(manager.sync_group().is_empty());
    }
}
