//! Typed form/domain mapping. Credentials are never filled from saved sessions.
use super::store::{
    kind_code, kind_of,
    session_config::{self, SessionConfig, SessionKind, SshAuth},
};
use crate::bridge::session::session_ffi::SessCfg;

/// 表单 POD → 领域配置（新建/保存共用）。空 id = 新建（store 侧生成稳定 id）。
pub fn pod_to_cfg(p: &SessCfg) -> SessionConfig {
    let mut c = SessionConfig::blank(kind_of(p.kind));
    c.id = p.id.clone();
    c.name = p.name.trim().to_string();
    c.meta.label = p.label.trim().to_string();
    c.meta.group_path = p.group_path.trim().to_string();
    c.meta.label_color = if p.label_color.is_empty() {
        "#22c55e".into()
    } else {
        p.label_color.clone()
    };
    c.meta.description = p.description.clone();
    c.meta.favorite = p.favorite;
    c.meta.terminal_emulation = if p.terminal_emulation.is_empty() {
        "xterm-256color".into()
    } else {
        p.terminal_emulation.clone()
    };
    c.meta.charset = if p.charset.is_empty() {
        "UTF-8".into()
    } else {
        p.charset.clone()
    };
    c.meta.system_shell = p.system_shell.clone();
    c.meta.sftp_enabled = p.sftp_enabled;
    c.meta.sysmon_enabled = p.sysmon_enabled;
    c.meta.options = super::session_advanced::from_pod(&p.options);
    match &mut c.kind {
        SessionKind::Local => {
            c.local = Some(session_config::LocalConfig {
                shell: p.local_shell.clone(),
                shell_path: p.local_path.clone(),
                args: p.local_args.clone(),
                working_directory: p.local_cwd.clone(),
            });
        }
        SessionKind::Ssh => {
            c.ssh = Some(session_config::SshConfig {
                host: p.ssh_host.trim().to_string(),
                port: p.ssh_port as u16,
                username: p.ssh_user.trim().to_string(),
                auth: match p.ssh_auth {
                    1 => SshAuth::Key,
                    2 => SshAuth::KeyboardInteractive,
                    3 => SshAuth::OneKey,
                    4 => SshAuth::Agent,
                    _ => SshAuth::Password,
                },
                // 密码/passphrase 只进内存（core 持久化 skip_serializing）。
                password: p.ssh_password.clone(),
                private_key_path: p.ssh_key_path.trim().to_string(),
                passphrase: p.ssh_passphrase.clone(),
                account_id: p.ssh_account_id.clone(),
                connect_timeout_secs: if p.ssh_timeout_secs == 0 {
                    15
                } else {
                    p.ssh_timeout_secs
                },
                reconnect_enabled: p.ssh_reconnect,
                max_reconnect_attempts: p.ssh_reconnect_attempts,
                reconnect_interval_secs: p.ssh_reconnect_interval,
                remember_credentials: p.ssh_remember,
            });
            c.meta.onekey_account_id = if p.ssh_auth == 3 && !p.ssh_account_id.is_empty() {
                Some(p.ssh_account_id.clone())
            } else {
                None
            };
        }
        SessionKind::Telnet => {
            c.telnet = Some(session_config::TelnetConfig {
                host: p.telnet_host.trim().to_string(),
                port: p.telnet_port as u16,
                terminal_type: if p.telnet_term.is_empty() {
                    "xterm".into()
                } else {
                    p.telnet_term.clone()
                },
            });
        }
        SessionKind::Serial => {
            c.serial = Some(session_config::SerialConfig {
                path: p.serial_path.trim().to_string(),
                baud_rate: if p.serial_baud == 0 {
                    115200
                } else {
                    p.serial_baud
                },
                data_bits: if p.serial_bits == 0 {
                    8
                } else {
                    p.serial_bits as u8
                },
                stop_bits: if p.serial_stop == 0 {
                    1
                } else {
                    p.serial_stop as u8
                },
                parity: p.serial_parity,
                flow_control: p.serial_flow,
            });
        }
        SessionKind::Tcp => {
            c.tcp = Some(session_config::TcpConfig {
                host: p.tcp_host.trim().to_string(),
                port: p.tcp_port as u16,
                terminal_type: if p.tcp_term.is_empty() {
                    "xterm".into()
                } else {
                    p.tcp_term.clone()
                },
                data_mode: u8::from(p.tcp_binary),
            });
        }
    }
    c
}

/// 领域配置 → 表单 POD（编辑回填；密码/passphrase 永不回填）。
pub fn cfg_to_pod(c: &SessionConfig) -> SessCfg {
    let mut p = SessCfg {
        options: super::session_advanced::to_pod(&c.meta.options),
        id: c.id.clone(),
        name: c.name.clone(),
        kind: kind_code(c.kind),
        group_path: c.meta.group_path.clone(),
        label: c.meta.label.clone(),
        label_color: c.meta.label_color.clone(),
        description: c.meta.description.clone(),
        favorite: c.meta.favorite,
        terminal_emulation: c.meta.terminal_emulation.clone(),
        charset: c.meta.charset.clone(),
        system_shell: c.meta.system_shell.clone(),
        sftp_enabled: c.meta.sftp_enabled,
        sysmon_enabled: c.meta.sysmon_enabled,
        local_shell: String::new(),
        local_path: String::new(),
        local_args: String::new(),
        local_cwd: String::new(),
        ssh_host: String::new(),
        ssh_port: 22,
        ssh_user: String::new(),
        ssh_auth: 0,
        ssh_password: String::new(),
        ssh_key_path: String::new(),
        ssh_account_id: String::new(),
        ssh_passphrase: String::new(),
        ssh_timeout_secs: 15,
        ssh_reconnect: false,
        ssh_reconnect_attempts: 3,
        ssh_reconnect_interval: 2,
        ssh_remember: false,
        telnet_host: String::new(),
        telnet_port: 23,
        telnet_term: "xterm".into(),
        serial_path: String::new(),
        serial_baud: 115200,
        serial_bits: 8,
        serial_stop: 1,
        serial_parity: 0,
        serial_flow: 0,
        tcp_host: String::new(),
        tcp_port: 23,
        tcp_term: "xterm".into(),
        tcp_binary: false,
    };
    if let Some(l) = &c.local {
        p.local_shell = l.shell.clone();
        p.local_path = l.shell_path.clone();
        p.local_args = l.args.clone();
        p.local_cwd = l.working_directory.clone();
    }
    if let Some(s) = &c.ssh {
        p.ssh_host = s.host.clone();
        p.ssh_port = s.port as u32;
        p.ssh_user = s.username.clone();
        p.ssh_auth = match s.auth {
            SshAuth::Password => 0,
            SshAuth::Key => 1,
            SshAuth::KeyboardInteractive => 2,
            SshAuth::OneKey => 3,
            SshAuth::Agent => 4,
        };
        p.ssh_key_path = s.private_key_path.clone();
        p.ssh_account_id = s.account_id.clone();
        p.ssh_timeout_secs = if s.connect_timeout_secs == 0 {
            15
        } else {
            s.connect_timeout_secs
        };
        p.ssh_reconnect = s.reconnect_enabled;
        p.ssh_reconnect_attempts = s.max_reconnect_attempts;
        p.ssh_reconnect_interval = s.reconnect_interval_secs;
        p.ssh_remember = s.remember_credentials;
    }
    if let Some(t) = &c.telnet {
        p.telnet_host = t.host.clone();
        p.telnet_port = t.port as u32;
        p.telnet_term = t.terminal_type.clone();
    }
    if let Some(s) = &c.serial {
        p.serial_path = s.path.clone();
        p.serial_baud = s.baud_rate;
        p.serial_bits = s.data_bits as u32;
        p.serial_stop = s.stop_bits as u32;
        p.serial_parity = s.parity;
        p.serial_flow = s.flow_control;
    }
    if let Some(t) = &c.tcp {
        p.tcp_host = t.host.clone();
        p.tcp_port = t.port as u32;
        p.tcp_term = t.terminal_type.clone();
        p.tcp_binary = t.data_mode == 1;
    }
    p
}
