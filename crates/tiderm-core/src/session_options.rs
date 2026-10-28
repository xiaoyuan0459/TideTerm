//! Session policies shared by persistence and live transports. Defaults preserve old sessions.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionOptions {
    pub bell_visual: bool,
    pub bell_sound: bool,
    pub bell_interval_secs: u32,
    pub auto_wrap: bool,
    pub newline_mode: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub cursor_style: u8,
    pub scrollback_mb: u32,
    pub backspace_del: bool,
    /// 0 = CSI 3~, 1 = DEL, 2 = BS; independent of Backspace.
    pub delete_mode: u8,
    pub enter_mode: u8,
    pub remote_clipboard: bool,
    pub remote_title: bool,
    pub log_enabled: bool,
    pub copy_on_select: bool,
    /// Double-click separates path components at slash/backslash when enabled.
    pub path_word_selection: bool,
    /// Save changed pane mark rules as defaults on an explicit UI close.
    pub mark_save_on_close: bool,
    pub right_click_paste: bool,
    pub modem_timeout_secs: u32,
    pub ssh: SshOptions,
    pub sftp_initial_dir: String,
    pub sftp_show_hidden: bool,
    pub sftp_follow_cwd: bool,
    pub sftp_repair_names: bool,
    pub sftp_preserve_modified: bool,
    pub sftp_preserve_accessed: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            bell_visual: true,
            bell_sound: false,
            bell_interval_secs: 2,
            auto_wrap: true,
            newline_mode: false,
            app_cursor: false,
            app_keypad: false,
            cursor_style: 0,
            scrollback_mb: 0, // zero inherits the application memory budget
            backspace_del: true,
            delete_mode: 0,
            enter_mode: 0,
            remote_clipboard: true,
            remote_title: true,
            log_enabled: false,
            copy_on_select: true,
            path_word_selection: false,
            mark_save_on_close: false,
            right_click_paste: false,
            modem_timeout_secs: 10,
            ssh: SshOptions::default(),
            sftp_initial_dir: String::new(),
            sftp_show_hidden: true,
            sftp_follow_cwd: true,
            sftp_repair_names: true,
            sftp_preserve_modified: true,
            sftp_preserve_accessed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SshOptions {
    pub proxy: ProxyOptions,
    pub compression: bool,
    pub keepalive_secs: u32,
    pub watchdog_secs: u32,
    pub ciphers: String,
    pub kex: String,
    pub macs: String,
    pub host_keys: String,
    pub agent_forwarding: bool,
    pub x11_enabled: bool,
    pub x11_display: String,
    pub x11_auth_file: String,
    pub x11_no_auth: bool,
    pub sftp_file_mode: u32,
    pub sftp_dir_mode: u32,
}

impl Default for SshOptions {
    fn default() -> Self {
        Self {
            proxy: ProxyOptions::default(),
            compression: false,
            keepalive_secs: 0,
            watchdog_secs: 120,
            ciphers: String::new(),
            kex: String::new(),
            macs: String::new(),
            host_keys: String::new(),
            agent_forwarding: false,
            x11_enabled: false,
            x11_display: String::new(),
            x11_auth_file: String::new(),
            x11_no_auth: false,
            sftp_file_mode: 0o644,
            sftp_dir_mode: 0o755,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyOptions {
    /// 0 direct, 1 SOCKS5 (remote DNS), 2 HTTP CONNECT.
    pub kind: u8,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
}

impl std::fmt::Debug for ProxyOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyOptions")
            .field("kind", &self.kind)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl SessionOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.cursor_style > 2
            || self.enter_mode > 2
            || self.delete_mode > 2
            || self.scrollback_mb > 512
            || self.bell_interval_secs > 60
        {
            return Err("invalid terminal options".into());
        }
        let s = &self.ssh;
        if !(1..=120).contains(&self.modem_timeout_secs) {
            return Err("Modem timeout must be 1–120 seconds".into());
        }
        if s.x11_display.len() > 255
            || s.x11_display.contains(['\r', '\n', '\0'])
            || s.x11_auth_file.len() > 4096
            || s.x11_auth_file.contains('\0')
        {
            return Err("invalid X11 display or authority path".into());
        }
        if s.sftp_file_mode > 0o777 || s.sftp_dir_mode > 0o777 {
            return Err("invalid SFTP creation permissions".into());
        }
        if s.keepalive_secs > 3600 || s.watchdog_secs > 600 {
            return Err("invalid SSH keepalive/watchdog interval".into());
        }
        let p = &s.proxy;
        if p.kind > 2 || (p.kind != 0 && (p.host.trim().is_empty() || p.port == 0)) {
            return Err("proxy requires a host and port".into());
        }
        if p.host.contains(['\r', '\n', '\0']) || p.username.len() > 255 || p.password.len() > 255 {
            return Err("invalid proxy address or credentials".into());
        }
        for list in [&s.ciphers, &s.kex, &s.macs, &s.host_keys] {
            if list.len() > 4096
                || (!list.is_empty()
                    && list.split(',').any(|v| {
                        v.is_empty()
                            || !v
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"-_.@+".contains(&c))
                    }))
            {
                return Err("SSH algorithms must be a comma-separated name list".into());
            }
        }
        if self.sftp_initial_dir.len() > 4096 || self.sftp_initial_dir.contains(['\0', '\r', '\n'])
        {
            return Err("invalid SFTP initial directory".into());
        }
        Ok(())
    }
}
