//! TCP (raw socket) configuration — connection parameters for TCP sessions.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

fn default_tcp_port() -> u16 {
    42
}

fn default_terminal_type() -> String {
    "xterm-256color".into()
}

fn default_connect_timeout_secs() -> u64 {
    30
}

fn default_data_mode() -> String {
    "binary".into()
}

/// TCP connection parameters used by the live connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpConfig {
    pub host: String,
    #[serde(default = "default_tcp_port")]
    pub port: u16,
    #[serde(default = "default_terminal_type")]
    pub terminal_type: String,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    /// `binary` (pass-through) or `text` (CRLF translation on send).
    #[serde(default = "default_data_mode")]
    pub data_mode: String,
}

impl TcpConfig {
    pub fn validate(&self) -> Result<()> {
        if self.host.trim().is_empty() {
            return Err(Error::Tcp("host is required".into()));
        }
        if self.terminal_type.trim().is_empty() {
            return Err(Error::Tcp("terminal_type is required".into()));
        }
        if self.connect_timeout_secs == 0 {
            return Err(Error::Tcp(
                "connect_timeout_secs must be greater than 0".into(),
            ));
        }
        Ok(())
    }
}

// ── Persisted session config (v0.9) ────────────────────────────
// De-shelled into `tiderm_core::config` (B2); re-exported here so
// `crate::protocol::tcp::TcpConnectionConfig` call-sites keep working.
pub use crate::config::TcpConnectionConfig;

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_config_serializes_round_trip() {
        let config = TcpConfig {
            host: "192.168.1.1".into(),
            port: 42,
            terminal_type: "xterm-256color".into(),
            connect_timeout_secs: 30,
            data_mode: "binary".into(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let decoded: TcpConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.host, "192.168.1.1");
        assert_eq!(decoded.port, 42);
    }

    #[test]
    fn tcp_config_deserializes_with_defaults() {
        let json = r#"{"host":"router.local"}"#;
        let config: TcpConfig = serde_json::from_str(json).expect("deserialize");
        assert_eq!(config.port, 42);
        assert_eq!(config.terminal_type, "xterm-256color");
        assert_eq!(config.data_mode, "binary");
        assert_eq!(config.connect_timeout_secs, 30);
    }

    #[test]
    fn tcp_config_rejects_empty_host() {
        let config = TcpConfig {
            host: "".into(),
            port: 42,
            terminal_type: "xterm-256color".into(),
            connect_timeout_secs: 30,
            data_mode: "binary".into(),
        };
        assert!(config.validate().is_err());
    }
}
