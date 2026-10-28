//! 会话配置持久化 glue（B2 之上的 native 侧）：`SessionConfig` ↔ core `SessionConfig`
//! 双向映射 + `SessionStore` 读写。侧栏会话树与「保存会话」共用此数据源。
//!
//! native 表单配置是轻量类型（无 id/name/时间戳）；持久化时由这里生成并映射到
//! core 的存储结构（`sessions.json`，与 Tauri 壳共用同一数据源（B2））。凭据
//! （password/passphrase）在 core 侧 `skip_serializing`，不明文落盘。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::session_config::{
    LocalConfig, SerialConfig, SessionConfig, SessionKind, SshAuth, SshConfig, TcpConfig,
    TelnetConfig,
};
use tiderm_core::config::{
    LocalShellConfig, SerialConnectionConfig, SessionMeta as CoreMeta, SessionStore,
    SshAuth as CoreSshAuth, SshConnectionConfig, TcpConnectionConfig, TelnetConnectionConfig,
};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成一个稳定的会话配置 id（时间戳 + 进程内自增，避免引入 uuid 依赖）。
fn gen_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("s-{ts}-{n}")
}

/// 本地时间的 ISO 格式（与 Tauri 壳 `sessions.json` 的 created_at/updated_at 兼容）。
fn iso_now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%.3f")
        .to_string()
}

fn kind_str(k: SessionKind) -> String {
    match k {
        SessionKind::Local => "local",
        SessionKind::Ssh => "ssh",
        SessionKind::Telnet => "telnet",
        SessionKind::Serial => "serial",
        SessionKind::Tcp => "tcp",
    }
    .into()
}

fn kind_from_str(s: &str) -> SessionKind {
    match s {
        "ssh" => SessionKind::Ssh,
        "telnet" => SessionKind::Telnet,
        "serial" => SessionKind::Serial,
        "tcp" => SessionKind::Tcp,
        _ => SessionKind::Local,
    }
}

/// native → core 认证方式（OneKey 走 KeyboardInteractive，凭据经 vault）。
fn to_core_auth(a: SshAuth, c: &SshConfig) -> CoreSshAuth {
    match a {
        SshAuth::Password => CoreSshAuth::Password {
            password: c.password.clone(),
        },
        SshAuth::Key => CoreSshAuth::PublicKey {
            private_key_path: c.private_key_path.clone(),
            passphrase: if c.passphrase.is_empty() {
                None
            } else {
                Some(c.passphrase.clone())
            },
        },
        SshAuth::KeyboardInteractive | SshAuth::OneKey => CoreSshAuth::KeyboardInteractive,
        SshAuth::Agent => CoreSshAuth::Agent,
    }
}

fn from_core_auth(a: &CoreSshAuth) -> (SshAuth, String, String) {
    match a {
        CoreSshAuth::Agent => (SshAuth::Agent, String::new(), String::new()),
        CoreSshAuth::Password { password } => (SshAuth::Password, password.clone(), String::new()),
        CoreSshAuth::PublicKey {
            private_key_path: _,
            passphrase,
        } => (
            SshAuth::Key,
            String::new(),
            passphrase.clone().unwrap_or_default(),
        ),
        CoreSshAuth::KeyboardInteractive => {
            (SshAuth::KeyboardInteractive, String::new(), String::new())
        }
    }
}

/// native → core（持久化形态）。已加载配置保留其稳定 id；新表单配置生成 id。
pub fn to_core(c: &SessionConfig) -> tiderm_core::config::SessionConfig {
    let now = iso_now();
    let id = if c.id.is_empty() {
        gen_id()
    } else {
        c.id.clone()
    };
    let kind = kind_str(c.kind);
    let local = c.local.as_ref().map(|l| LocalShellConfig {
        shell: l.shell.clone(),
        shell_path: if l.shell_path.is_empty() {
            None
        } else {
            Some(l.shell_path.clone())
        },
        args: if l.args.is_empty() {
            Vec::new()
        } else {
            vec![l.args.clone()]
        },
        env: Default::default(),
        working_directory: l.working_directory.clone(),
    });
    let ssh = c.ssh.as_ref().map(|s| SshConnectionConfig {
        host: s.host.clone(),
        port: s.port,
        username: s.username.clone(),
        auth: to_core_auth(s.auth, s),
        connect_timeout_secs: s.connect_timeout_secs as u64,
        reconnect_enabled: s.reconnect_enabled,
        max_reconnect_attempts: s.max_reconnect_attempts,
        reconnect_interval_secs: s.reconnect_interval_secs as u64,
        remember_credentials: s.remember_credentials,
    });
    let telnet = c.telnet.as_ref().map(|t| TelnetConnectionConfig {
        host: t.host.clone(),
        port: t.port,
        terminal_type: t.terminal_type.clone(),
    });
    let tcp = c.tcp.as_ref().map(|t| TcpConnectionConfig {
        host: t.host.clone(),
        port: t.port,
        terminal_type: t.terminal_type.clone(),
        data_mode: if t.data_mode == 1 {
            "binary".into()
        } else {
            "text".into()
        },
    });
    let serial = c.serial.as_ref().map(|s| SerialConnectionConfig {
        path: s.path.clone(),
        baud_rate: s.baud_rate,
        data_bits: s.data_bits,
        stop_bits: s.stop_bits,
        parity: match s.parity {
            1 => "odd",
            2 => "even",
            _ => "none",
        }
        .into(),
        flow_control: match s.flow_control {
            1 => "hardware",
            2 => "xonxoff",
            _ => "none",
        }
        .into(),
    });
    let meta = Some(CoreMeta {
        options: c.meta.options.clone(),
        onekey_account_id: c.ssh.as_ref().and_then(|ssh| {
            (ssh.auth == SshAuth::OneKey && !ssh.account_id.is_empty())
                .then(|| ssh.account_id.clone())
        }),
        label_text: c.meta.label.clone(),
        label_color: if c.meta.label_color.is_empty() {
            None
        } else {
            Some(c.meta.label_color.clone())
        },
        label_icon: if c.meta.label_icon.is_empty() {
            None
        } else {
            Some(c.meta.label_icon.clone())
        },
        group_path: if c.meta.group_path.is_empty() {
            None
        } else {
            Some(c.meta.group_path.clone())
        },
        data_mode: c.meta.data_mode.clone(),
        terminal_emulation: if c.meta.terminal_emulation.is_empty() {
            "xterm-256color".into()
        } else {
            c.meta.terminal_emulation.clone()
        },
        system_shell: c.meta.system_shell.clone(),
        charset: c.meta.charset.clone(),
        description: c.meta.description.clone(),
        sftp_enabled: c.meta.sftp_enabled,
        sysmon_enabled: c.meta.sysmon_enabled,
        favorite: c.meta.favorite,
    });
    let name = if c.name.is_empty() {
        c.display_name()
    } else {
        c.name.clone()
    };
    tiderm_core::config::SessionConfig {
        id,
        name,
        kind,
        group: None,
        tags: Vec::new(),
        sort_order: 0,
        local,
        ssh,
        telnet,
        serial,
        tcp,
        meta,
        created_at: now.clone(),
        updated_at: now,
    }
}

/// core → native（表单/侧栏展示形态，保留持久化 id）。
pub fn from_core(c: &tiderm_core::config::SessionConfig) -> SessionConfig {
    let kind = kind_from_str(&c.kind);
    let mut cfg = SessionConfig::blank(kind);
    cfg.meta.options = c
        .meta
        .as_ref()
        .map(|m| m.options.clone())
        .unwrap_or_default();
    cfg.id = c.id.clone();
    cfg.name = c.name.clone();
    cfg.meta.label = c
        .meta
        .as_ref()
        .map(|m| m.label_text.clone())
        .unwrap_or_default();
    cfg.meta.group_path = c
        .meta
        .as_ref()
        .and_then(|m| m.group_path.clone())
        .unwrap_or_default();
    cfg.meta.terminal_emulation = c
        .meta
        .as_ref()
        .map(|m| m.terminal_emulation.clone())
        .unwrap_or_default();
    cfg.meta.charset = c
        .meta
        .as_ref()
        .map(|m| m.charset.clone())
        .unwrap_or_default();
    cfg.meta.description = c
        .meta
        .as_ref()
        .map(|m| m.description.clone())
        .unwrap_or_default();
    let meta_ref = c.meta.as_ref();
    cfg.meta.label_color = meta_ref
        .and_then(|m| m.label_color.clone())
        .unwrap_or_else(|| "#22c55e".into());
    cfg.meta.label_icon = meta_ref
        .and_then(|m| m.label_icon.clone())
        .unwrap_or_default();
    cfg.meta.onekey_account_id = meta_ref.and_then(|m| m.onekey_account_id.clone());
    cfg.meta.data_mode = meta_ref
        .map(|m| m.data_mode.clone())
        .unwrap_or_else(|| "binary".into());
    cfg.meta.system_shell = meta_ref
        .map(|m| m.system_shell.clone())
        .unwrap_or_else(|| "cmd".into());
    cfg.meta.sftp_enabled = meta_ref.map(|m| m.sftp_enabled).unwrap_or(true);
    cfg.meta.sysmon_enabled = meta_ref.map(|m| m.sysmon_enabled).unwrap_or(false);
    cfg.meta.favorite = meta_ref.map(|m| m.favorite).unwrap_or(false);

    if let Some(s) = &c.ssh {
        let (mut auth, password, passphrase) = from_core_auth(&s.auth);
        let account_id = cfg.meta.onekey_account_id.clone().unwrap_or_default();
        if !account_id.is_empty() && matches!(s.auth, CoreSshAuth::KeyboardInteractive) {
            auth = SshAuth::OneKey;
        }
        cfg.ssh = Some(SshConfig {
            host: s.host.clone(),
            port: s.port,
            username: s.username.clone(),
            auth,
            password,
            private_key_path: match &s.auth {
                CoreSshAuth::PublicKey {
                    private_key_path, ..
                } => private_key_path.clone(),
                _ => String::new(),
            },
            passphrase,
            account_id,
            connect_timeout_secs: s.connect_timeout_secs as u32,
            reconnect_enabled: s.reconnect_enabled,
            max_reconnect_attempts: s.max_reconnect_attempts,
            reconnect_interval_secs: s.reconnect_interval_secs as u32,
            remember_credentials: s.remember_credentials,
        });
    }
    if let Some(t) = &c.telnet {
        cfg.telnet = Some(TelnetConfig {
            host: t.host.clone(),
            port: t.port,
            terminal_type: t.terminal_type.clone(),
        });
    }
    if let Some(t) = &c.tcp {
        cfg.tcp = Some(TcpConfig {
            host: t.host.clone(),
            port: t.port,
            terminal_type: t.terminal_type.clone(),
            data_mode: if t.data_mode == "binary" { 1 } else { 0 },
        });
    }
    if let Some(s) = &c.serial {
        cfg.serial = Some(SerialConfig {
            path: s.path.clone(),
            baud_rate: s.baud_rate,
            data_bits: s.data_bits,
            stop_bits: s.stop_bits,
            parity: match s.parity.as_str() {
                "odd" => 1,
                "even" => 2,
                _ => 0,
            },
            flow_control: match s.flow_control.as_str() {
                "hardware" => 1,
                "xonxoff" => 2,
                _ => 0,
            },
        });
    }
    if let Some(l) = &c.local {
        cfg.local = Some(LocalConfig {
            shell: l.shell.clone(),
            shell_path: l.shell_path.clone().unwrap_or_default(),
            args: l.args.join(" "),
            working_directory: l.working_directory.clone(),
        });
    }
    cfg
}

/// 持久化保存一个会话配置。
pub fn save(c: &SessionConfig) -> Result<(), String> {
    let core = to_core(c);
    let mut store = SessionStore::load().map_err(|e| e.to_string())?;
    store.upsert(core);
    store.save().map_err(|e| e.to_string())
}

/// 全部已保存会话（core → native）。
pub fn list() -> Vec<SessionConfig> {
    match SessionStore::load() {
        Ok(store) => store.sessions().iter().map(from_core).collect(),
        Err(_) => Vec::new(),
    }
}

/// Move a persisted session before another persisted session.
pub fn move_before(dragged_id: &str, target_id: &str) -> Result<bool, String> {
    let mut store = SessionStore::load().map_err(|e| e.to_string())?;
    move_before_in(&mut store, dragged_id, target_id)
}

/// `move_before` 的纯 store 版本（hermetic tests 使用 temp path）。
pub fn move_before_in(
    store: &mut SessionStore,
    dragged_id: &str,
    target_id: &str,
) -> Result<bool, String> {
    if !store.move_before(dragged_id, target_id) {
        return Ok(false);
    }
    store.save().map_err(|e| e.to_string())?;
    Ok(true)
}

/// 重命名已保存会话（按稳定 id）。
pub fn rename(id: &str, new_name: &str) -> Result<(), String> {
    let mut store = SessionStore::load().map_err(|e| e.to_string())?;
    rename_in(&mut store, id, new_name)
}

/// `rename` 的纯 store 版本（hermetic tests 使用 temp path）。
/// 切换会话收藏标记（G18：快捷连接栏「收藏的」分组）。
pub fn set_favorite(id: &str, favorite: bool) -> Result<(), String> {
    let mut store = SessionStore::load().map_err(|e| e.to_string())?;
    let mut cfg = store
        .find(id)
        .cloned()
        .ok_or_else(|| format!("会话不存在：{id}"))?;
    let meta = cfg.meta.get_or_insert_with(Default::default);
    meta.favorite = favorite;
    store.upsert(cfg);
    store.save().map_err(|e| e.to_string())
}

/// 按 id 读取单个会话（无则 None）。
pub fn get(id: &str) -> Option<SessionConfig> {
    list().into_iter().find(|c| c.id == id)
}

pub fn rename_in(store: &mut SessionStore, id: &str, new_name: &str) -> Result<(), String> {
    let mut cfg = store
        .find(id)
        .cloned()
        .ok_or_else(|| format!("会话不存在：{id}"))?;
    cfg.name = new_name.to_string();
    store.upsert(cfg);
    store.save().map_err(|e| e.to_string())
}

/// 删除已保存会话（按稳定 id），返回是否存在并已删除。
pub fn delete(id: &str) -> Result<bool, String> {
    let mut store = SessionStore::load().map_err(|e| e.to_string())?;
    delete_in(&mut store, id)
}

/// `delete` 的纯 store 版本（hermetic tests 使用 temp path）。
pub fn delete_in(store: &mut SessionStore, id: &str) -> Result<bool, String> {
    let removed = store.remove(id);
    if removed {
        store.save().map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "session_store_tests.rs"]
mod tests;
