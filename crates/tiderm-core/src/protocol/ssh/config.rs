//! SSH configuration types — connection parameters and authentication methods.

use serde::{Deserialize, Serialize};

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

fn default_link_watchdog_secs() -> u64 {
    120
}

fn default_ssh_term() -> String {
    "xterm-256color".into()
}

/// SSH connection parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    #[serde(default)]
    pub options: crate::session_options::SshOptions,
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
    /// PTY 请求的终端模拟类型（会话表单「终端:(T)」，默认 xterm-256color）。
    #[serde(default = "default_ssh_term")]
    pub term: String,
    /// 静默链路看门狗（R-16）：**本地有键入、却迟迟收不到任何字节**超过这个秒数，
    /// 就把会话判死（`alive=false` → 壳层显示已断开、可走自动重连）。
    ///
    /// This echo watchdog is independent of the configurable active keepalive.
    /// `0` = 关闭看门狗。
    #[serde(default = "default_link_watchdog_secs")]
    pub link_watchdog_secs: u64,
}

/// SSH authentication methods.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_config_password_serializes_round_trip() {
        let config = SshConfig {
            host: "192.168.1.1".into(),
            port: 22,
            username: "admin".into(),
            auth: SshAuth::Password {
                password: "secret".into(),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            // 与 core 默认一致（120 s）：静默链路看门狗
            link_watchdog_secs: 120,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let decoded: SshConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.host, "192.168.1.1");
        match decoded.auth {
            SshAuth::Password { password } => assert_eq!(password, ""),
            _ => panic!("expected Password auth"),
        }
    }

    #[test]
    fn ssh_config_password_not_persisted() {
        let config = SshConfig {
            host: "host".into(),
            port: 22,
            username: "user".into(),
            auth: SshAuth::Password {
                password: "leak-me-if-you-can".into(),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            // 与 core 默认一致（120 s）：静默链路看门狗
            link_watchdog_secs: 120,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(!json.contains("leak-me-if-you-can"));
        assert!(json.contains("\"type\":\"password\""));
        assert!(!json.contains("\"password\":"));
    }

    #[test]
    fn ssh_config_passphrase_not_persisted() {
        let config = SshConfig {
            host: "host".into(),
            port: 22,
            username: "user".into(),
            auth: SshAuth::PublicKey {
                private_key_path: "/key".into(),
                passphrase: Some("leak-me-if-you-can".into()),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            // 与 core 默认一致（120 s）：静默链路看门狗
            link_watchdog_secs: 120,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(!json.contains("leak-me-if-you-can"));
        assert!(!json.contains("passphrase"));
    }

    #[test]
    fn ssh_config_reconnect_fields_deserialize_with_defaults() {
        let json =
            r#"{"host":"h","port":22,"username":"u","auth":{"type":"password","password":"p"}}"#;
        let config: SshConfig = serde_json::from_str(json).expect("deserialize");
        assert_eq!(config.connect_timeout_secs, 30);
        assert!(!config.reconnect_enabled);
        assert_eq!(config.max_reconnect_attempts, 3);
        assert_eq!(config.reconnect_interval_secs, 2);
    }
}
