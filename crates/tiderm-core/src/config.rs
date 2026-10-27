//! De-shelled session-config persistence (B2).
//!
//! The persisted session-config types and a [`SessionStore`] (save / read / delete /
//! create) live here so both the Tauri app and the native shell share one source of
//! truth. Field names use `snake_case` to match the TypeScript `SessionConfig`
//! interface exactly. Credentials (password / passphrase / API keys) are never
//! persisted — they are `#[serde(skip_serializing)]` so they survive only in memory.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Persisted session configuration (a saved connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sort_order: u32,
    pub local: Option<LocalShellConfig>,
    pub ssh: Option<SshConnectionConfig>,
    /// Telnet connection parameters (present when kind == "telnet").
    #[serde(default)]
    pub telnet: Option<TelnetConnectionConfig>,
    /// Serial port parameters (present when kind == "serial").
    #[serde(default)]
    pub serial: Option<SerialConnectionConfig>,
    /// TCP raw-socket parameters (present when kind == "tcp").
    #[serde(default)]
    pub tcp: Option<TcpConnectionConfig>,
    /// Shared session-level metadata (label, OneKey, toggles, …).
    #[serde(default)]
    pub meta: Option<SessionMeta>,
    pub created_at: String,
    pub updated_at: String,
}

/// Local shell parameters (present when kind == "local").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShellConfig {
    pub shell: String,
    pub shell_path: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_directory: String,
}

fn default_connect_timeout_secs() -> u64 {
    30
}
fn default_reconnect_enabled() -> bool {
    false
}
fn default_max_reconnect_attempts() -> u32 {
    3
}
fn default_reconnect_interval_secs() -> u64 {
    2
}

/// Persisted SSH connection parameters (present when kind == "ssh").
///
/// Distinct from the live `SshConfig` used by the connector; this is the stored form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,

    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_reconnect_enabled")]
    pub reconnect_enabled: bool,
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_reconnect_attempts: u32,
    #[serde(default = "default_reconnect_interval_secs")]
    pub reconnect_interval_secs: u64,
    /// Whether the user asked the shell to remember the SSH credential in the
    /// encrypted vault (AES-256-GCM `vault.json`). The flag itself is not a
    /// secret; the credential it refers to never serializes.
    #[serde(default)]
    pub remember_credentials: bool,
}

/// Persisted SSH authentication methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SshAuth {
    #[serde(rename = "password")]
    Password {
        // Credentials are never persisted to disk; they live only in memory.
        // skip_serializing (not skip) so IPC can still pass the password in.
        #[serde(skip_serializing, default)]
        password: String,
    },
    #[serde(rename = "key")]
    PublicKey {
        private_key_path: String,
        // Passphrase is never persisted to disk; it lives only in memory.
        #[serde(skip_serializing, default)]
        passphrase: Option<String>,
    },
    #[serde(rename = "keyboard-interactive")]
    KeyboardInteractive,
    #[serde(rename = "agent")]
    Agent,
}

fn default_telnet_port() -> u16 {
    23
}
fn default_terminal_type() -> String {
    "xterm-256color".into()
}

/// Persisted Telnet connection parameters (present when kind == "telnet").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelnetConnectionConfig {
    pub host: String,
    #[serde(default = "default_telnet_port")]
    pub port: u16,
    #[serde(default = "default_terminal_type")]
    pub terminal_type: String,
}

fn default_tcp_port() -> u16 {
    42
}
fn default_data_mode() -> String {
    "binary".into()
}

/// Persisted TCP raw-socket parameters (present when kind == "tcp").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TcpConnectionConfig {
    pub host: String,
    #[serde(default = "default_tcp_port")]
    pub port: u16,
    #[serde(default = "default_terminal_type")]
    pub terminal_type: String,
    #[serde(default = "default_data_mode")]
    pub data_mode: String,
}

fn default_baud_rate() -> u32 {
    115_200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".into()
}
fn default_flow_control() -> String {
    "none".into()
}

/// Persisted serial port parameters (present when kind == "serial").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerialConnectionConfig {
    pub path: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: String,
    #[serde(default = "default_flow_control")]
    pub flow_control: String,
}

/// Shared session-level metadata (v0.9+). Mirrors the TypeScript
/// `SessionMeta` interface (snake_case) so `meta` round-trips exactly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub options: crate::session_options::SessionOptions,
    /// OneKey account binding; `None` = "无" (no OneKey).
    #[serde(default)]
    pub onekey_account_id: Option<String>,
    /// Tab label shown next to the session name.
    #[serde(default)]
    pub label_text: String,
    /// Small color swatch next to the label field.
    #[serde(default)]
    pub label_color: Option<String>,
    /// Small icon preset key next to the label field (reference UI "ICON").
    #[serde(default)]
    pub label_icon: Option<String>,
    /// Nested group path joined with `>`. `None` = root group.
    #[serde(default)]
    pub group_path: Option<String>,
    /// Wire format for bytes: `binary` or `text`.
    #[serde(default = "default_meta_data_mode")]
    pub data_mode: String,
    /// Terminal emulation announced to the remote side.
    #[serde(default = "default_meta_terminal_emulation")]
    pub terminal_emulation: String,
    /// System shell profile (informational for remote sessions).
    #[serde(default = "default_meta_system_shell")]
    pub system_shell: String,
    /// Character set used when decoding the incoming stream.
    #[serde(default = "default_meta_charset")]
    pub charset: String,
    /// Free-form description.
    #[serde(default)]
    pub description: String,
    /// SSH-only: whether SFTP is negotiated on connect.
    #[serde(default = "default_meta_sftp_enabled")]
    pub sftp_enabled: bool,
    /// Whether the Sysmon overlay is enabled for this session.
    #[serde(default)]
    pub sysmon_enabled: bool,
    /// User-favorited (quick-connect bar "收藏的" group). G18.
    #[serde(default)]
    pub favorite: bool,
}

fn default_meta_data_mode() -> String {
    "binary".into()
}
fn default_meta_terminal_emulation() -> String {
    "xterm-256color".into()
}
fn default_meta_system_shell() -> String {
    "cmd".into()
}
fn default_meta_charset() -> String {
    "UTF-8".into()
}
fn default_meta_sftp_enabled() -> bool {
    true
}

/// A de-shelled session-config store persisted to `{config_dir}/sessions.json`.
///
/// Provides the B2 public API — save / read / delete / create — that both the Tauri
/// app and the native shell call. The store owns a `Vec<SessionConfig>` and writes
/// atomically (temp file + rename) to avoid a partially-written file on crash.
pub struct SessionStore {
    sessions: Vec<SessionConfig>,
    path: PathBuf,
}

impl SessionStore {
    /// The file path used for the session store.
    pub fn default_path() -> crate::error::Result<PathBuf> {
        Ok(crate::config_dir()?.join("sessions.json"))
    }

    /// Create an empty store backed by an arbitrary path (used by the app
    /// fallback path and by hermetic tests).
    pub fn new(path: PathBuf) -> Self {
        Self {
            sessions: Vec::new(),
            path,
        }
    }

    /// The path this store persists to.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Load the store from the default `sessions.json` location.
    pub fn load() -> crate::error::Result<Self> {
        Self::load_at(Self::default_path()?)
    }

    /// Load the store from an explicit path, returning an empty store when
    /// the file is absent.
    ///
    /// A corrupted file is backed up to `sessions.json.bak` before returning
    /// empty, so a later `save()` does not silently overwrite the damaged data.
    pub fn load_at(path: PathBuf) -> crate::error::Result<Self> {
        let mut sessions: Vec<SessionConfig> = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(data) => match serde_json::from_str(&data) {
                    Ok(list) => list,
                    Err(e) => {
                        let backup = path.with_extension("json.bak");
                        let _ = std::fs::copy(&path, &backup);
                        tracing::error!(
                            "session store parse failed (backed up to {:?}): {e}",
                            backup
                        );
                        Vec::new()
                    }
                },
                Err(e) => return Err(format!("read session store: {e}").into()),
            }
        } else {
            Vec::new()
        };
        let mut ids = std::collections::HashSet::new();
        let original_len = sessions.len();
        sessions.retain(|session| ids.insert(session.id.clone()));
        if sessions.len() != original_len {
            // Loading must not fail because a best-effort forensic backup could
            // not be written (read-only profile, full disk, ACL denial).
            let backup = path.with_extension("json.duplicates.bak");
            if let Err(error) = std::fs::copy(&path, &backup) {
                tracing::warn!(%error, "duplicate session ids removed; backup failed");
            }
            tracing::warn!(
                dropped = original_len - sessions.len(),
                "duplicate session ids removed"
            );
        }
        Ok(Self { sessions, path })
    }

    /// Persist the store to disk atomically, creating parent directories as needed.
    pub fn save(&self) -> crate::error::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.sessions)
            .map_err(|e| format!("serialize session store: {e}"))?;
        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    /// Find a session config by ID.
    pub fn find(&self, id: &str) -> Option<&SessionConfig> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Insert or update a session config (upsert by ID).
    pub fn upsert(&mut self, session: SessionConfig) {
        if let Some(existing) = self.sessions.iter_mut().find(|s| s.id == session.id) {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
    }

    /// Move `id` immediately before `target_id`, renumbering `sort_order`.
    /// Returns `false` if either id is missing.
    pub fn move_before(&mut self, id: &str, target_id: &str) -> bool {
        let from = self.sessions.iter().position(|s| s.id == id);
        let to = self.sessions.iter().position(|s| s.id == target_id);
        let (Some(from), Some(to)) = (from, to) else {
            return false;
        };
        if from == to {
            return true;
        }
        let item = self.sessions.remove(from);
        let insert_at = if from < to { to - 1 } else { to };
        self.sessions.insert(insert_at, item);
        for (i, session) in self.sessions.iter_mut().enumerate() {
            session.sort_order = i as u32;
        }
        true
    }

    /// Remove a session config by ID, returning `true` when it was present.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|s| s.id != id);
        self.sessions.len() != before
    }

    /// All persisted session configs.
    pub fn sessions(&self) -> &[SessionConfig] {
        &self.sessions
    }

    /// Number of persisted session configs.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the store has no sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
