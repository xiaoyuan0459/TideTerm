//! Telnet configuration — connection parameters for Telnet sessions.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

fn default_connect_timeout_secs() -> u64 {
    30
}

fn default_terminal_type() -> String {
    "xterm-256color".into()
}

/// Telnet connection parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelnetConfig {
    pub host: String,
    #[serde(default = "default_telnet_port")]
    pub port: u16,
    #[serde(default = "default_terminal_type")]
    pub terminal_type: String,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
}

impl TelnetConfig {
    pub fn validate(&self) -> Result<()> {
        if self.host.trim().is_empty() {
            return Err(Error::Telnet("host is required".into()));
        }
        if self.terminal_type.trim().is_empty() {
            return Err(Error::Telnet("terminal_type is required".into()));
        }
        if self.connect_timeout_secs == 0 {
            return Err(Error::Telnet(
                "connect_timeout_secs must be greater than 0".into(),
            ));
        }
        Ok(())
    }
}

fn default_telnet_port() -> u16 {
    23
}

// ── Persisted session config (v0.5) ────────────────────────────
// De-shelled into `tiderm_core::config` (B2); re-exported here so
// `crate::protocol::telnet::TelnetConnectionConfig` call-sites keep working.
pub use crate::config::TelnetConnectionConfig;

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telnet_config_serializes_round_trip() {
        let config = TelnetConfig {
            host: "192.168.1.1".into(),
            port: 23,
            terminal_type: "xterm-256color".into(),
            connect_timeout_secs: 30,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let decoded: TelnetConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.host, "192.168.1.1");
        assert_eq!(decoded.port, 23);
        assert_eq!(decoded.terminal_type, "xterm-256color");
    }

    #[test]
    fn telnet_config_deserializes_with_defaults() {
        let json = r#"{"host":"router.local"}"#;
        let config: TelnetConfig = serde_json::from_str(json).expect("deserialize");
        assert_eq!(config.port, 23);
        assert_eq!(config.terminal_type, "xterm-256color");
        assert_eq!(config.connect_timeout_secs, 30);
    }

    #[test]
    fn telnet_config_rejects_empty_host() {
        let config = TelnetConfig {
            host: "".into(),
            port: 23,
            terminal_type: "xterm-256color".into(),
            connect_timeout_secs: 30,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn telnet_config_rejects_empty_terminal_type() {
        let config = TelnetConfig {
            host: "router.local".into(),
            port: 23,
            terminal_type: "".into(),
            connect_timeout_secs: 30,
        };
        assert!(config.validate().is_err());
    }
}
