//! session 域桥（ADR-014 §Decision 3「按域拆桥」第三域，Phase C ④）。
//!
//! 会话持久化 CRUD + 连接树快照 + 真实会话打开 + SSH 交互提示（主机密钥
//! 确认 / keyboard-interactive）。实现符号顶层 `pub use` 先于桥声明（E0425
//! 规约）；`sess_` 前缀跨桥全局唯一；只传 POD。

pub use crate::rust::events::{answer_host_key, take_prompt};
pub use crate::rust::session_explorer::sess_explorer_rows;
pub use crate::rust::store::{cfg_to_pod, pod_to_cfg, tree_rows};
pub use crate::rust::term::{open_tab_with_config, respond_kb};

/// `sess_open` 失败哨兵（cxx 桥不便传 Option<u32>）。
pub const OPEN_FAILED: u32 = u32::MAX;
/// `sess_open` 要求先解锁凭据库（vault 有记住的凭据但本进程未解锁）。
/// C++ 弹解锁框后重试；用户取消也返回此值。C++ 侧常量 `chrome::kVaultLocked`
/// 必须与本值一致（cxx 不导出 Rust 常量）。
pub const VAULT_LOCKED: u32 = u32::MAX - 1;

pub use crate::rust::session_advanced::{sess_ssh_algorithms, sess_validate};
pub use crate::rust::session_defaults::{sess_defaults_get, sess_defaults_save};

#[cxx::bridge]
pub mod session_ffi {
    pub struct SessOptions {
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
        pub delete_mode: u8,
        pub enter_mode: u8,
        pub remote_clipboard: bool,
        pub remote_title: bool,
        pub log_enabled: bool,
        pub copy_on_select: bool,
        pub path_word_selection: bool,
        pub mark_save_on_close: bool,
        pub right_click_paste: bool,
        pub modem_timeout_secs: u32,
        pub compression: bool,
        pub keepalive_secs: u32,
        pub watchdog_secs: u32,
        pub proxy_kind: u8,
        pub proxy_host: String,
        pub proxy_port: u32,
        pub proxy_user: String,
        pub proxy_password: String,
        pub ciphers: String,
        pub kex: String,
        pub macs: String,
        pub host_keys: String,
        pub agent_forwarding: bool,
        pub x11_enabled: bool,
        pub x11_display: String,
        pub x11_auth_file: String,
        pub x11_no_auth: bool,
        pub sftp_initial_dir: String,
        pub sftp_show_hidden: bool,
        pub sftp_follow_cwd: bool,
        pub sftp_repair_names: bool,
        pub sftp_preserve_modified: bool,
        pub sftp_preserve_accessed: bool,
        pub sftp_file_mode: u32,
        pub sftp_dir_mode: u32,
    }
    /// 表单配置（五协议字段全平铺——cxx 不收 Option/嵌套枚举；未用协议字段空）。
    /// 密码/passphrase 只进内存：core 持久化 skip_serializing，回填永远为空。
    pub struct SessCfg {
        pub options: SessOptions,
        pub id: String,
        pub name: String,
        /// 0=local 1=ssh 2=telnet 3=serial 4=tcp。
        pub kind: u8,
        pub group_path: String,
        pub label: String,
        pub label_color: String,
        pub description: String,
        pub favorite: bool,
        // 共享元数据（SessionMeta）：终端:(T) + 字符集:(C) 用户可编辑；
        // SFTP channel policy and asynchronous system monitoring.
        pub terminal_emulation: String,
        pub charset: String,
        pub system_shell: String,
        pub sftp_enabled: bool,
        pub sysmon_enabled: bool,
        // local
        pub local_shell: String,
        pub local_path: String,
        pub local_args: String,
        pub local_cwd: String,
        // ssh（auth：0=password 1=key 2=keyboard-interactive 3=onekey）
        pub ssh_host: String,
        pub ssh_port: u32,
        pub ssh_user: String,
        pub ssh_auth: u8,
        pub ssh_password: String,
        pub ssh_key_path: String,
        pub ssh_account_id: String,
        pub ssh_passphrase: String,
        pub ssh_timeout_secs: u32,
        pub ssh_reconnect: bool,
        pub ssh_reconnect_attempts: u32,
        pub ssh_reconnect_interval: u32,
        /// 记住密码：凭据存主密码加密的 vault.json（vault 域桥）。
        pub ssh_remember: bool,
        // telnet
        pub telnet_host: String,
        pub telnet_port: u32,
        pub telnet_term: String,
        // serial（parity/flow：0 none · 1 odd/hardware · 2 even/xonxoff）
        pub serial_path: String,
        pub serial_baud: u32,
        pub serial_bits: u32,
        pub serial_stop: u32,
        pub serial_parity: u8,
        pub serial_flow: u8,
        // tcp
        pub tcp_host: String,
        pub tcp_port: u32,
        pub tcp_term: String,
        pub tcp_binary: bool,
    }

    /// 连接树行（与 pod 域 PodRow 同形但独立类型——cxx 跨桥同名 struct 是
    /// 两个 C++ 类型，共享 POD 归主桥的规约不适用于纯快照行）。
    pub struct SessRow {
        pub id: String,
        pub label: String,
        /// 0=分组 1=会话。
        pub kind: u8,
        pub depth: u32,
        pub protocol: String,
        pub status: String,
    }

    /// 待 UI 应答的交互提示（host key / keyboard-interactive 弹窗数据）。
    pub struct PromptInfo {
        /// 0=hostkey 1=kb。
        pub kind: u8,
        pub uid: String,
        /// hostkey：host:port；kb：挑战名。
        pub title: String,
        /// hostkey：key_type；kb：instruction。
        pub detail: String,
        /// hostkey：SHA-256 指纹；kb：空。
        pub fingerprint: String,
        /// hostkey 状态：0=Known 1=Unknown 2=KeyChanged（KeyChanged 必须醒目警示）。
        pub state: u8,
        /// kb 字段提示文本（与 echoes 平行）。
        pub fields: Vec<String>,
        /// kb 字段是否回显（false = 密码框）。
        pub echoes: Vec<bool>,
    }

    extern "Rust" {
        fn sess_ssh_algorithms(group: u8) -> Vec<String>;
        fn sess_validate(cfg: &SessCfg) -> String;
        /// 连接树快照（sessions.json 分组树 DFS 拍平；收藏分组置顶）。
        fn sess_tree_rows() -> Vec<SessRow>;
        fn sess_explorer_rows() -> Vec<SessRow>;
        /// 编辑回填：按 id 取表单 POD；不存在时返回空 id 的 blank(kind)。
        fn sess_form_get(id: &str, kind: u8) -> SessCfg;
        fn sess_defaults_get(kind: u8) -> SessCfg;
        fn sess_defaults_save(cfg: &SessCfg) -> String;
        /// 当前窗格的完整配置，用于「保存会话」编辑现有或快速会话。
        fn sess_active_config(idx: u32) -> SessCfg;
        /// 保存（空 id = 新建）。返回稳定 id。
        fn sess_save(cfg: &SessCfg) -> String;
        /// 删除持久会话。
        fn sess_delete(id: &str) -> bool;
        /// 重命名。
        fn sess_rename(id: &str, name: &str) -> bool;
        /// 拖拽排序（dragged 移到 target 前）。
        fn sess_move_before(from: &str, to: &str) -> bool;
        /// 收藏开关。
        fn sess_set_favorite(id: &str, fav: bool) -> bool;
        /// 复制会话（新 id + 「(副本)」后缀）；返回新 id，空 = 失败。
        fn sess_duplicate(id: &str) -> String;
        /// 导出会话为 JSON 文件；返回错误文本，空 = 成功。
        fn sess_export(id: &str, path: &str) -> String;
        /// 导入 JSON 文件（逐条重生成 id）；返回条数，-1 = 读取/解析失败。
        fn sess_import(path: &str) -> i64;
        /// 拖拽改组/排序（pos 0=before 1=after 2=inside）；空 = 成功，非空 = 错误文本。
        fn sess_drag_move(dragged: &str, target: &str, pos: u8) -> String;
        /// 打开持久会话 → 新标签（五协议）。u32::MAX = 无此会话。
        fn sess_open(id: &str) -> u32;
        /// 快速连接（不落盘）：kind + host/port/user；返回新标签 id。
        fn sess_quick_open(kind: u8, host: &str, port: u32, user: &str, password: &str) -> u32;
        fn sess_open_draft(cfg: &SessCfg) -> u32;
        fn sess_quick_saved(host: &str, port: u32, user: &str) -> String;
        /// 取最早待应答提示（无 = kind 0）。C++ 轮询弹窗。
        fn sess_take_prompt() -> PromptInfo;
        /// 应答主机密钥确认（accept=false 拒绝即断开）。
        fn sess_answer_host_key(uid: &str, accept: bool) -> bool;
        /// 应答 keyboard-interactive 挑战。
        fn sess_answer_kb(uid: &str, responses: Vec<String>) -> bool;
        /// 自检钩子：sessions.json 重定向到临时目录（c4 hermetic，防污染用户数据）。
        fn sess_test_store_dir(path: &str);
        /// 自检钩子：恢复默认 store 路径。
        fn sess_test_store_reset();
    }
}

use crate::rust::events::PendingPrompt;
use crate::rust::store::session_config::SessionKind;
use session_ffi::{PromptInfo, SessCfg, SessRow};

fn kind_of(code: u8) -> SessionKind {
    match code {
        1 => SessionKind::Ssh,
        2 => SessionKind::Telnet,
        3 => SessionKind::Serial,
        4 => SessionKind::Tcp,
        _ => SessionKind::Local,
    }
}

pub(crate) fn sess_tree_rows() -> Vec<SessRow> {
    tree_rows()
}

pub(crate) fn sess_form_get(id: &str, kind: u8) -> SessCfg {
    if !id.is_empty()
        && let Some(c) = crate::rust::store::get_by_id(id)
    {
        return cfg_to_pod(&c);
    }
    sess_defaults_get(kind)
}

pub(crate) fn sess_active_config(idx: u32) -> SessCfg {
    cfg_to_pod(&crate::rust::term_lifecycle::active_config(idx))
}

pub(crate) fn sess_save(cfg: &SessCfg) -> String {
    if let Err(error) = crate::rust::session_advanced::validate(&pod_to_cfg(cfg)) {
        eprintln!("[session] invalid options: {error}");
        return String::new();
    }
    match crate::rust::store::save_returning_id(&pod_to_cfg(cfg)) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("[session] save failed: {error}");
            String::new()
        }
    }
}

pub(crate) fn sess_delete(id: &str) -> bool {
    let removed = crate::rust::store::delete_by_id(id);
    if removed {
        // 凭据删除不解密 vault，锁定状态下也能清理。
        crate::rust::vault::remove_session_credentials(id);
    }
    removed
}

pub(crate) fn sess_rename(id: &str, name: &str) -> bool {
    crate::rust::store::rename_by_id(id, name).is_ok()
}

pub(crate) fn sess_move_before(from: &str, to: &str) -> bool {
    crate::rust::store::move_before_by_id(from, to)
}

pub(crate) fn sess_set_favorite(id: &str, fav: bool) -> bool {
    crate::rust::store::set_favorite_by_id(id, fav).is_ok()
}

pub(crate) fn sess_duplicate(id: &str) -> String {
    match crate::rust::session_io::duplicate(id) {
        Ok(new_id) => {
            // 记住的凭据随副本走（解锁态下复制；未解锁跳过，尽力而为）。
            crate::rust::vault::copy_session_credentials(id, &new_id);
            new_id
        }
        Err(error) => {
            eprintln!("[session] duplicate failed: {error}");
            String::new()
        }
    }
}

pub(crate) fn sess_export(id: &str, path: &str) -> String {
    match crate::rust::session_io::export(id, path) {
        Ok(()) => String::new(),
        Err(error) => error,
    }
}

pub(crate) fn sess_drag_move(dragged: &str, target: &str, pos: u8) -> String {
    match crate::rust::session_io::drag_move(dragged, target, pos) {
        Ok(()) => String::new(),
        Err(error) => error,
    }
}

pub(crate) fn sess_import(path: &str) -> i64 {
    match crate::rust::session_io::import(path) {
        Ok(n) => i64::try_from(n).unwrap_or(i64::MAX),
        Err(error) => {
            eprintln!("[session] import failed: {error}");
            -1
        }
    }
}

pub(crate) fn sess_open(id: &str) -> u32 {
    if crate::rust::term_ops::input_locked() {
        return OPEN_FAILED;
    }
    match crate::rust::store::get_by_id_checked(id) {
        Ok(Some(mut cfg)) => {
            // 记住密码：连接前从凭据库注入（VAULT_LOCKED = 先解锁再重试）。
            match crate::rust::vault::inject_saved_credentials(&mut cfg) {
                crate::rust::vault::Inject::Locked => VAULT_LOCKED,
                crate::rust::vault::Inject::Ready => open_tab_with_config(&cfg),
            }
        }
        _ => OPEN_FAILED,
    }
}

pub(crate) fn sess_quick_open(kind: u8, host: &str, port: u32, user: &str, password: &str) -> u32 {
    let mut pod = sess_form_get("", kind);
    match kind_of(kind) {
        SessionKind::Ssh => {
            pod.ssh_host = host.to_string();
            pod.ssh_port = if port == 0 { 22 } else { port };
            pod.ssh_user = user.to_string();
            pod.ssh_password = password.to_string();
            pod.name = format!("{user}@{host}");
        }
        SessionKind::Telnet => {
            pod.telnet_host = host.to_string();
            pod.telnet_port = if port == 0 { 23 } else { port };
            pod.name = format!("telnet {host}");
        }
        SessionKind::Tcp => {
            pod.tcp_host = host.to_string();
            pod.tcp_port = if port == 0 { 23 } else { port };
            pod.name = format!("tcp {host}");
        }
        SessionKind::Serial => {
            pod.serial_path = host.to_string();
            pod.name = format!("serial {host}");
        }
        SessionKind::Local => {
            // 快速连接对本地无意义——退化为默认本地 shell。
            pod.name = "本地".into();
        }
    }
    let _ = password;
    open_tab_with_config(&pod_to_cfg(&pod))
}

pub(crate) fn sess_open_draft(cfg: &SessCfg) -> u32 {
    open_tab_with_config(&pod_to_cfg(cfg))
}

pub(crate) fn sess_quick_saved(host: &str, port: u32, user: &str) -> String {
    crate::rust::store::list_all()
        .into_iter()
        .find_map(|cfg| {
            let ssh = cfg.ssh.as_ref()?;
            (ssh.host.eq_ignore_ascii_case(host)
                && u32::from(ssh.port) == if port == 0 { 22 } else { port }
                && (user.is_empty() || ssh.username == user)
                && ssh.remember_credentials)
                .then_some(cfg.id)
        })
        .unwrap_or_default()
}

pub(crate) fn sess_take_prompt() -> PromptInfo {
    match take_prompt() {
        None => PromptInfo {
            kind: 0,
            uid: String::new(),
            title: String::new(),
            detail: String::new(),
            fingerprint: String::new(),
            state: 0,
            fields: Vec::new(),
            echoes: Vec::new(),
        },
        Some(PendingPrompt::HostKey {
            uid,
            host,
            port,
            key_type,
            fingerprint,
            state,
        }) => PromptInfo {
            kind: 1,
            uid,
            title: format!("{host}:{port}"),
            detail: key_type,
            fingerprint,
            state,
            fields: Vec::new(),
            echoes: Vec::new(),
        },
        Some(PendingPrompt::Kb {
            uid,
            name,
            instruction,
            prompts,
        }) => PromptInfo {
            kind: 2,
            uid,
            title: name,
            detail: instruction,
            fingerprint: String::new(),
            state: 0,
            fields: prompts.iter().map(|(p, _)| p.clone()).collect(),
            echoes: prompts.iter().map(|(_, e)| *e).collect(),
        },
    }
}

pub(crate) fn sess_answer_host_key(uid: &str, accept: bool) -> bool {
    answer_host_key(uid, accept)
}

pub(crate) fn sess_answer_kb(uid: &str, responses: Vec<String>) -> bool {
    respond_kb(uid, responses).is_ok()
}

pub(crate) fn sess_test_store_dir(path: &str) {
    crate::rust::store::set_store_dir(Some(std::path::PathBuf::from(path)));
}

pub(crate) fn sess_test_store_reset() {
    crate::rust::store::set_store_dir(None);
}
