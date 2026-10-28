//! 会话配置持久化层（Phase C ④）——`session_config`（纯数据模型）+
//! `session_store`（core SessionStore glue）。
//!
//! 两个文件读写 `sessions.json`（用户配置目录），凭据由 core 侧
//! `skip_serializing` 保证不落盘。桥面 POD（[`crate::bridge::session`] 的
//! `SessCfg`）在本文件做双向映射——cxx 不收 Option/嵌套枚举，表单字段全平铺。

#[path = "shared/session_config.rs"]
pub mod session_config;
#[path = "shared/session_store.rs"]
pub mod session_store;

#[cfg(test)]
use crate::bridge::session::session_ffi::SessCfg;
pub use session_config::{SessionConfig, SessionKind};

// ════════════════════ store 访问层（自检可重定向，生产走默认路径） ════════════════════

use std::path::PathBuf;
use std::sync::Mutex;
use tiderm_core::config::SessionStore;

static STORE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 重定向 sessions.json 所在目录（c4 自检 hermetic 用；None = 用户默认目录）。
pub fn set_store_dir(dir: Option<PathBuf>) {
    *STORE_DIR.lock().unwrap() = dir;
}

pub(super) fn state_directory() -> Result<PathBuf, String> {
    if let Some(dir) = STORE_DIR.lock().map_err(|_| "session store lock")?.clone() {
        return Ok(dir);
    }
    #[cfg(test)]
    {
        // Automatic recording and workspace tests must never touch the desktop profile.
        static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        Ok(ROOT
            .get_or_init(|| tempfile::tempdir().expect("isolated native state directory"))
            .path()
            .into())
    }
    #[cfg(not(test))]
    tiderm_core::config_dir().map_err(|e| e.to_string())
}

pub(super) fn defaults_path() -> PathBuf {
    load_store().path().with_file_name("session-defaults.json")
}

fn try_load_store() -> Result<SessionStore, String> {
    let dir = STORE_DIR.lock().map_err(|_| "session store lock")?.clone();
    let path = match dir {
        Some(d) => d.join("sessions.json"),
        None => SessionStore::default_path().map_err(|e| e.to_string())?,
    };
    SessionStore::load_at(path).map_err(|e| e.to_string())
}
fn load_store() -> SessionStore {
    try_load_store().expect("session store")
}

pub fn list_all_checked() -> Result<Vec<SessionConfig>, String> {
    Ok(try_load_store()?
        .sessions()
        .iter()
        .map(session_store::from_core)
        .collect())
}
pub fn get_by_id_checked(id: &str) -> Result<Option<SessionConfig>, String> {
    Ok(try_load_store()?.find(id).map(session_store::from_core))
}

/// 保存并返回稳定 id（to_core 先生成 id → upsert → save）。
pub fn save_returning_id(c: &SessionConfig) -> Result<String, String> {
    super::session_advanced::validate(c)?;
    let core = session_store::to_core(c);
    let id = core.id.clone();
    let mut store = load_store();
    store.upsert(core);
    store.save().map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn list_all() -> Vec<SessionConfig> {
    load_store()
        .sessions()
        .iter()
        .map(session_store::from_core)
        .collect()
}

pub fn get_by_id(id: &str) -> Option<SessionConfig> {
    load_store().find(id).map(session_store::from_core)
}

pub fn delete_by_id(id: &str) -> bool {
    let mut store = load_store();
    let removed = store.remove(id);
    if removed {
        let _ = store.save();
    }
    removed
}

pub fn rename_by_id(id: &str, name: &str) -> Result<(), String> {
    let mut store = load_store();
    let mut cfg = store
        .find(id)
        .cloned()
        .ok_or_else(|| format!("会话不存在：{id}"))?;
    cfg.name = name.to_string();
    store.upsert(cfg);
    store.save().map_err(|e| e.to_string())
}

pub fn move_before_by_id(from: &str, to: &str) -> bool {
    let mut store = load_store();
    if store.move_before(from, to) {
        store.save().is_ok()
    } else {
        false
    }
}

/// 以给定顺序**整体重写** store（拖拽改组/排序用：save 原子落盘）。
/// `SessionStore` 只暴露 `move_before`，整体重排需按序 upsert 到新 store。
pub fn rewrite_ordered(ordered: &[SessionConfig]) -> Result<(), String> {
    let path = load_store().path().to_path_buf();
    let mut next = SessionStore::new(path);
    for c in ordered {
        next.upsert(session_store::to_core(c));
    }
    next.save().map_err(|e| e.to_string())
}

pub fn set_favorite_by_id(id: &str, fav: bool) -> Result<(), String> {
    let mut store = load_store();
    let mut cfg = store
        .find(id)
        .cloned()
        .ok_or_else(|| format!("会话不存在：{id}"))?;
    let meta = cfg.meta.get_or_insert_with(Default::default);
    meta.favorite = fav;
    store.upsert(cfg);
    store.save().map_err(|e| e.to_string())
}

// ════════════════════ 表单 POD ↔ SessionConfig ════════════════════

pub(super) fn kind_code(k: SessionKind) -> u8 {
    match k {
        SessionKind::Local => 0,
        SessionKind::Ssh => 1,
        SessionKind::Telnet => 2,
        SessionKind::Serial => 3,
        SessionKind::Tcp => 4,
    }
}

pub(super) fn kind_of(code: u8) -> SessionKind {
    match code {
        1 => SessionKind::Ssh,
        2 => SessionKind::Telnet,
        3 => SessionKind::Serial,
        4 => SessionKind::Tcp,
        _ => SessionKind::Local,
    }
}

pub use super::session_form_config::{cfg_to_pod, pod_to_cfg};

// ════════════════════ 连接树（sessions.json → DFS 拍平） ════════════════════

use crate::bridge::session::session_ffi::SessRow;

/// 连接树快照：持久配置按 `group_path`（`>` 分隔）聚成嵌套分组，DFS 拍平
/// （分组在前、子节点紧随；C++ PodTreeModel 按 depth 栈重建层级）。
/// 排序：收藏置顶分组「收藏的」，其余按 store 序（用户拖拽序）。
/// 分组路径段解析（参考 `services/sessionTree.ts:6,17-27 parseGroupPath`）：
/// 分隔 → trim → 去空段 → **硬钳 3 层**（多余段丢弃，会话挂在第 3 层分组下）。
/// Qt 侧分隔符是 `>`（会话表单字段契约），其余语义与参考同源。
pub const MAX_GROUP_DEPTH: usize = 3;

pub(crate) fn group_segments(raw: &str) -> Vec<String> {
    let mut parts: Vec<String> = raw
        .split('>')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .map(str::to_string)
        .collect();
    parts.truncate(MAX_GROUP_DEPTH);
    parts
}

pub fn tree_rows() -> Vec<SessRow> {
    let sessions = list_all();
    let mut out = Vec::new();
    // 收藏分组（G18 对齐 Slint 壳快捷连接栏）
    let favs: Vec<&SessionConfig> = sessions.iter().filter(|c| c.meta.favorite).collect();
    if !favs.is_empty() {
        out.push(SessRow {
            id: "group:收藏的".into(),
            label: "收藏的".into(),
            kind: 0,
            depth: 0,
            protocol: String::new(),
            status: String::new(),
        });
        for c in favs {
            out.push(sess_row(c, 1));
        }
    }
    // 分组路径 → 前缀树（保持 store 出现序）
    let mut groups: Vec<Vec<String>> = Vec::new(); // 已发射的分组路径栈（去重用）
    for c in &sessions {
        let path = group_segments(&c.meta.group_path);
        for i in 0..path.len() {
            let prefix: Vec<String> = path[..=i].to_vec();
            if !groups.contains(&prefix) {
                groups.push(prefix.clone());
                let label = prefix.last().cloned().unwrap_or_default();
                let id = format!("group:{}", prefix.join("/"));
                out.push(SessRow {
                    id,
                    label,
                    kind: 0,
                    depth: i as u32,
                    protocol: String::new(),
                    status: String::new(),
                });
            }
        }
        let depth = path.len() as u32;
        out.push(sess_row(c, depth));
    }
    out
}

fn sess_row(c: &SessionConfig, depth: u32) -> SessRow {
    SessRow {
        id: c.id.clone(),
        label: if c.name.is_empty() {
            c.display_name()
        } else {
            c.name.clone()
        },
        kind: 1,
        depth,
        protocol: kind_str(c.kind).into(),
        status: String::new(),
    }
}

fn kind_str(k: SessionKind) -> &'static str {
    match k {
        SessionKind::Local => "local",
        SessionKind::Ssh => "ssh",
        SessionKind::Telnet => "telnet",
        SessionKind::Serial => "serial",
        SessionKind::Tcp => "tcp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_pod(host: &str, group: &str) -> SessCfg {
        let mut p = cfg_to_pod(&SessionConfig::blank(SessionKind::Ssh));
        p.name = "prod".into();
        p.ssh_host = host.into();
        p.ssh_user = "root".into();
        p.group_path = group.into();
        p
    }

    #[test]
    fn pod_cfg_round_trip_keeps_form_fields() {
        let mut p = ssh_pod("10.0.0.1", "生产 > 数据库");
        p.ssh_auth = 3;
        p.ssh_account_id = "ok-production".into();
        let c = pod_to_cfg(&p);
        assert_eq!(c.kind, SessionKind::Ssh);
        assert_eq!(c.ssh.as_ref().unwrap().host, "10.0.0.1");
        assert_eq!(c.meta.onekey_account_id.as_deref(), Some("ok-production"));
        assert_eq!(c.meta.group_path, "生产 > 数据库");
        let back = cfg_to_pod(&c);
        assert_eq!(back.ssh_host, "10.0.0.1");
        assert_eq!(back.ssh_user, "root");
        assert_eq!(back.ssh_account_id, "ok-production");
        assert_eq!(back.group_path, "生产 > 数据库");
        // 密码永不回填
        assert!(back.ssh_password.is_empty());
    }

    #[test]
    fn group_path_trims_blanks_and_caps_at_three_levels() {
        assert_eq!(group_segments(" 生产 > > 数据库 "), vec!["生产", "数据库"]);
        assert_eq!(group_segments(""), Vec::<String>::new());
        // 参考硬钳：第 4 段起丢弃，会话落在第 3 层分组下（不再往更深嵌套）。
        assert_eq!(
            group_segments("a > b > c > d > e"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn tree_rows_nest_by_group_path() {
        // 纯函数部分只验分组拍平形状（不落盘：tree_rows 读 default store，
        // 这里用内部形状断言替代——见 c4 离屏自检的端到端版）。
        let p = ssh_pod("h", "A > B");
        let c = pod_to_cfg(&p);
        let row = sess_row(&c, 2);
        assert_eq!(row.depth, 2);
        assert_eq!(row.protocol, "ssh");
    }
}
