//! Self-built session-config data model + field validation (T2.2).
//!
//! Fields mirror what the Qt session form (`crate::bridge::session`) collects and what
//! `store::session_store` persists into `sessions.json`; mounted into
//! `crate::rust::store` via `#[path]`.
//!
//! `dead_code` is expected here: this is a data contract whose fields are consumed by the
//! backend (B2), not by the current temporary bootstrap.
#![allow(dead_code)]

#[path = "display_names.rs"]
mod display_names;
pub use display_names::{shell_display_name, standard_shell_name};

/// The five session protocols the form can create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Local,
    Ssh,
    Telnet,
    Serial,
    Tcp,
}

impl SessionKind {
    pub const ALL: [SessionKind; 5] = [
        SessionKind::Local,
        SessionKind::Ssh,
        SessionKind::Telnet,
        SessionKind::Serial,
        SessionKind::Tcp,
    ];

    /// Stable label shown in the form's protocol tabs.
    pub fn label(self) -> &'static str {
        match self {
            SessionKind::Local => "本地",
            SessionKind::Ssh => "SSH",
            SessionKind::Telnet => "Telnet",
            SessionKind::Serial => "串口",
            SessionKind::Tcp => "TCP",
        }
    }
}

/// SSH authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshAuth {
    Password,
    Key,
    KeyboardInteractive,
    OneKey,
    Agent,
}

/// Shared connection metadata (present for every protocol).
///
/// 字段与 `tiderm_core::config::SessionMeta` / 旧 WebView `SessionMeta` 一一对应
/// （见 `docs/webview-parity` 的 SessionForm 章节），默认值对齐旧版
/// `DEFAULT_SESSION_META`。
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub options: tiderm_core::session_options::SessionOptions,
    /// 标签文字（旧 `meta.label_text`，与会话名 `SessionConfig::name` 是两个字段）。
    pub label: String,
    /// 标签颜色（旧 `meta.label_color`，默认 `#22c55e`）。
    pub label_color: String,
    /// 标签图标预设（旧 `meta.label_icon`，空 = 无）。
    pub label_icon: String,
    /// 嵌套分组路径，用 `>` 分隔（旧 `meta.group_path`）。
    pub group_path: String,
    /// OneKey 账户绑定（旧 `meta.onekey_account_id`）。
    pub onekey_account_id: Option<String>,
    /// 传输模式：`binary` / `text`（旧 `meta.data_mode`）。
    pub data_mode: String,
    pub terminal_emulation: String,
    pub system_shell: String,
    pub charset: String,
    pub description: String,
    /// SSH 专属：连接时协商 SFTP 子系统。
    pub sftp_enabled: bool,
    /// Sysmon 悬浮监控开关。
    pub sysmon_enabled: bool,
    /// 用户收藏（快捷连接栏「收藏的」分组，G18）。
    pub favorite: bool,
}

impl Default for SessionMeta {
    fn default() -> Self {
        SessionMeta {
            options: Default::default(),
            label: String::new(),
            label_color: "#22c55e".into(),
            label_icon: String::new(),
            group_path: String::new(),
            onekey_account_id: None,
            data_mode: "binary".into(),
            terminal_emulation: "xterm-256color".into(),
            system_shell: "cmd".into(),
            charset: "UTF-8".into(),
            description: String::new(),
            sftp_enabled: true,
            sysmon_enabled: false,
            favorite: false,
        }
    }
}

/// SSH connection options.
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    pub password: String,
    pub private_key_path: String,
    pub passphrase: String,
    pub account_id: String,
    pub connect_timeout_secs: u32,
    pub reconnect_enabled: bool,
    /// 最大重连次数（旧 `max_reconnect_attempts`，0–100，默认 3）。
    pub max_reconnect_attempts: u32,
    /// 重连间隔秒数（旧 `reconnect_interval_secs`，1–300，默认 2）。
    pub reconnect_interval_secs: u32,
    /// 用户勾选「记住密码」：凭据存主密码加密的 vault.json（AES-256-GCM）。
    /// 标志本身不是机密；它指向的凭据永不序列化（core skip_serializing）。
    pub remember_credentials: bool,
}

/// Telnet connection options.
#[derive(Debug, Clone)]
pub struct TelnetConfig {
    pub host: String,
    pub port: u16,
    pub terminal_type: String,
}

/// Serial connection options.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: u8,       // 0 none · 1 odd · 2 even
    pub flow_control: u8, // 0 none · 1 hardware · 2 xonxoff
}

/// Raw TCP connection options.
#[derive(Debug, Clone)]
pub struct TcpConfig {
    pub host: String,
    pub port: u16,
    pub terminal_type: String,
    pub data_mode: u8, // 0 text · 1 binary
}

/// Local shell options.
#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub shell: String,
    pub shell_path: String,
    pub args: String,
    pub working_directory: String,
}

/// A complete session configuration for the SessionForm.
///
/// `id` is empty for newly composed forms; persisted configs loaded from
/// `sessions.json` carry their stable id so the UI can rename/delete them.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub id: String,
    /// 会话名称（旧 `SessionConfig.name`，与 `meta.label` 是两个字段）。
    pub name: String,
    pub kind: SessionKind,
    pub ssh: Option<SshConfig>,
    pub telnet: Option<TelnetConfig>,
    pub serial: Option<SerialConfig>,
    pub tcp: Option<TcpConfig>,
    pub local: Option<LocalConfig>,
    pub meta: SessionMeta,
}

impl SessionConfig {
    /// A blank config for `kind` with protocol defaults (sensible ports, etc.).
    pub fn blank(kind: SessionKind) -> Self {
        let mut c = SessionConfig {
            id: String::new(),
            name: String::new(),
            kind,
            ssh: None,
            telnet: None,
            serial: None,
            tcp: None,
            local: None,
            meta: SessionMeta::default(),
        };
        match kind {
            SessionKind::Ssh => c.ssh = Some(SshConfig::default()),
            SessionKind::Telnet => {
                c.telnet = Some(TelnetConfig {
                    host: String::new(),
                    port: 23,
                    terminal_type: "xterm".into(),
                })
            }
            SessionKind::Serial => {
                c.serial = Some(SerialConfig {
                    path: String::new(),
                    baud_rate: 115200,
                    data_bits: 8,
                    stop_bits: 1,
                    parity: 0,
                    flow_control: 0,
                })
            }
            SessionKind::Tcp => {
                c.tcp = Some(TcpConfig {
                    host: String::new(),
                    port: 23,
                    terminal_type: "xterm".into(),
                    data_mode: 0,
                })
            }
            SessionKind::Local => {
                c.local = Some(LocalConfig {
                    shell: String::new(),
                    shell_path: String::new(),
                    args: String::new(),
                    working_directory: String::new(),
                })
            }
        }
        c
    }

    /// 会话显示名（侧栏/标签/会话头共用）：名称 → 标签 → 按协议字段派生。
    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if !self.meta.label.is_empty() {
            return self.meta.label.clone();
        }
        match &self.kind {
            SessionKind::Ssh => self
                .ssh
                .as_ref()
                .map(|s| {
                    if s.username.is_empty() {
                        s.host.clone()
                    } else {
                        format!("{}@{}", s.username, s.host)
                    }
                })
                .unwrap_or_else(|| "SSH".into()),
            SessionKind::Telnet => self
                .telnet
                .as_ref()
                .map(|t| t.host.clone())
                .unwrap_or_else(|| "Telnet".into()),
            SessionKind::Serial => self
                .serial
                .as_ref()
                .map(|s| s.path.clone())
                .unwrap_or_else(|| "串口".into()),
            SessionKind::Tcp => self
                .tcp
                .as_ref()
                .map(|t| t.host.clone())
                .unwrap_or_else(|| "TCP".into()),
            SessionKind::Local => self
                .local
                .as_ref()
                .filter(|local| !matches!(local.shell.as_str(), "" | "auto"))
                .map(|local| shell_display_name(&local.shell))
                .unwrap_or_else(|| "Shell".into()),
        }
    }

    /// 会话头协议徽标小写字符串（"local"/"ssh"/"telnet"/"serial"/"tcp"）。
    pub fn protocol_str(&self) -> &'static str {
        match self.kind {
            SessionKind::Local => "local",
            SessionKind::Ssh => "ssh",
            SessionKind::Telnet => "telnet",
            SessionKind::Serial => "serial",
            SessionKind::Tcp => "tcp",
        }
    }

    /// 会话头主机文本（local 为空串；serial 显示设备路径）。
    pub fn host_str(&self) -> String {
        match &self.kind {
            SessionKind::Ssh => self
                .ssh
                .as_ref()
                .map(|s| s.host.clone())
                .unwrap_or_default(),
            SessionKind::Telnet => self
                .telnet
                .as_ref()
                .map(|t| t.host.clone())
                .unwrap_or_default(),
            SessionKind::Tcp => self
                .tcp
                .as_ref()
                .map(|t| t.host.clone())
                .unwrap_or_default(),
            SessionKind::Serial => self
                .serial
                .as_ref()
                .map(|s| s.path.clone())
                .unwrap_or_default(),
            SessionKind::Local => String::new(),
        }
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        SshConfig {
            host: String::new(),
            port: 22,
            username: String::new(),
            auth: SshAuth::Password,
            password: String::new(),
            private_key_path: String::new(),
            passphrase: String::new(),
            account_id: String::new(),
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            remember_credentials: false,
        }
    }
}

/// Validates a config and returns human-readable field errors (empty = valid).
pub fn validate(c: &SessionConfig) -> Vec<String> {
    let mut errs = Vec::new();
    match c.kind {
        SessionKind::Ssh => {
            let s = c.ssh.as_ref().expect("ssh config");
            if s.host.trim().is_empty() {
                errs.push("主机名不能为空".to_string());
            }
            if s.port == 0 {
                errs.push("端口无效".to_string());
            }
            if s.username.trim().is_empty() {
                errs.push("用户名不能为空".to_string());
            }
            if s.auth == SshAuth::Key && s.private_key_path.trim().is_empty() {
                errs.push("私钥路径不能为空".to_string());
            }
        }
        SessionKind::Telnet | SessionKind::Tcp => {
            let host = match c.kind {
                SessionKind::Telnet => &c.telnet.as_ref().expect("telnet").host,
                _ => &c.tcp.as_ref().expect("tcp").host,
            };
            let port = match c.kind {
                SessionKind::Telnet => c.telnet.as_ref().expect("telnet").port,
                _ => c.tcp.as_ref().expect("tcp").port,
            };
            if host.trim().is_empty() {
                errs.push("主机名不能为空".to_string());
            }
            if port == 0 {
                errs.push("端口无效".to_string());
            }
        }
        SessionKind::Serial => {
            let s = c.serial.as_ref().expect("serial");
            if s.path.trim().is_empty() {
                errs.push("串口路径不能为空".to_string());
            }
            if s.baud_rate == 0 {
                errs.push("波特率无效".to_string());
            }
            if !matches!(s.data_bits, 5..=8) {
                errs.push("数据位需在 5–8".to_string());
            }
            if !matches!(s.stop_bits, 1 | 2) {
                errs.push("停止位需为 1 或 2".to_string());
            }
        }
        SessionKind::Local => {
            // Local always has a working default shell; nothing strictly required.
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_ssh_has_default_port() {
        let c = SessionConfig::blank(SessionKind::Ssh);
        assert_eq!(c.ssh.as_ref().unwrap().port, 22);
    }

    #[test]
    fn ssh_requires_host_and_username() {
        let c = SessionConfig::blank(SessionKind::Ssh);
        let errs = validate(&c);
        assert!(errs.iter().any(|e| e.contains("主机名")));
        assert!(errs.iter().any(|e| e.contains("用户名")));
    }

    #[test]
    fn ssh_key_auth_requires_key_path() {
        let mut c = SessionConfig::blank(SessionKind::Ssh);
        let s = c.ssh.as_mut().unwrap();
        s.host = "example.com".into();
        s.username = "root".into();
        s.auth = SshAuth::Key;
        let errs = validate(&c);
        assert!(errs.iter().any(|e| e.contains("私钥")));
    }

    #[test]
    fn telnet_requires_host() {
        let c = SessionConfig::blank(SessionKind::Telnet);
        assert!(validate(&c).iter().any(|e| e.contains("主机名")));
    }

    #[test]
    fn serial_requires_path_and_valid_parity() {
        let mut c = SessionConfig::blank(SessionKind::Serial);
        let s = c.serial.as_mut().unwrap();
        s.path = "COM3".into();
        s.baud_rate = 9600;
        s.parity = 1; // odd — valid
        assert!(validate(&c).is_empty());
    }

    #[test]
    fn serial_rejects_zero_baud() {
        let mut c = SessionConfig::blank(SessionKind::Serial);
        c.serial.as_mut().unwrap().path = "COM3".into();
        c.serial.as_mut().unwrap().baud_rate = 0;
        assert!(validate(&c).iter().any(|e| e.contains("波特率")));
    }

    #[test]
    fn local_is_always_valid() {
        let c = SessionConfig::blank(SessionKind::Local);
        assert!(validate(&c).is_empty());
    }

    #[test]
    fn tcp_requires_host() {
        let c = SessionConfig::blank(SessionKind::Tcp);
        assert!(validate(&c).iter().any(|e| e.contains("主机名")));
    }

    #[test]
    fn serial_rejects_invalid_data_and_stop_bits() {
        let mut c = SessionConfig::blank(SessionKind::Serial);
        let s = c.serial.as_mut().unwrap();
        s.path = "COM3".into();
        s.baud_rate = 9600;
        s.data_bits = 9;
        s.stop_bits = 3;
        let errs = validate(&c);
        assert!(errs.iter().any(|e| e.contains("数据位")));
        assert!(errs.iter().any(|e| e.contains("停止位")));
    }

    #[test]
    fn valid_ssh_config_passes_validation() {
        let mut c = SessionConfig::blank(SessionKind::Ssh);
        let s = c.ssh.as_mut().unwrap();
        s.host = "example.com".into();
        s.port = 22;
        s.username = "root".into();
        s.auth = SshAuth::Password;
        assert!(validate(&c).is_empty());
    }
}
