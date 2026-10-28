//! 会话树的「复制 / 导出 / 导入」（对齐批次 8 · 清单 §4.1）。
//!
//! 参考实现：`src/app/session-config-helpers.ts:14-78`（export/import/duplicate）
//! 与 `src/components/SessionTree/index.ts:366-376`（右键菜单项）。三条「读→改→写」
//! 全在本模块完成，C++ 只负责对话框与提示——铁律：C++ 不持有业务状态。
//!
//! 凭据：core 侧 `skip_serializing` 使密码/私钥口令永不落盘，故导出的 JSON
//! 天然不含密钥（与参考行为一致，参考导出的也是内存中的持久态）。

use crate::rust::store::{self, session_store};

/// 复制会话：新 id + 名称后缀「 (副本)」，其余字段原样保留。
/// 返回新 id。参考 `duplicateSessionConfig`（`{...session, id: uuid, name: "x (副本)"}`）。
pub fn duplicate(id: &str) -> Result<String, String> {
    let mut copy = store::get_by_id(id).ok_or_else(|| format!("会话不存在：{id}"))?;
    copy.id.clear(); // 空 id → to_core 内 gen_id()
    copy.name = format!("{} (副本)", copy.name);
    store::save_returning_id(&copy)
}

/// 导出单条会话为 JSON 文件（形状 = `sessions.json` 的数组切片，可直接回导）。
pub fn export(id: &str, path: &str) -> Result<(), String> {
    let cfg = store::get_by_id(id).ok_or_else(|| format!("会话不存在：{id}"))?;
    let json = serde_json::to_string_pretty(&[session_store::to_core(&cfg)])
        .map_err(|e| format!("序列化失败：{e}"))?;
    std::fs::write(path, json).map_err(|e| format!("写入失败：{e}"))
}

/// 导入 JSON 文件（数组）。逐条**重生成 id** 后落盘，绝不覆盖既有会话。
/// 返回成功条数（0 = 文件里没有有效配置）。
pub fn import(path: &str) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取失败：{e}"))?;
    let list: Vec<tiderm_core::config::SessionConfig> =
        serde_json::from_str(&text).map_err(|e| format!("解析失败：{e}"))?;
    let mut n = 0;
    for item in list {
        let mut cfg = session_store::from_core(&item);
        cfg.id.clear();
        if store::save_returning_id(&cfg).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// 分组节点 id（`group:A/B`，由 `store::tree_rows` 以 '/' 拼接）→ 段列表。
/// 注意与 `group_path`（'>' 分隔）**不同源**，混用会把整串当成一层。
fn id_segments(raw: &str) -> Vec<String> {
    let mut parts: Vec<String> = raw
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    parts.truncate(store::MAX_GROUP_DEPTH);
    parts
}

/// 拖拽改组 / 组内排序（参考 `index.ts:265-290` + `drag-drop.ts:31-37`）。
///
/// `pos`：0=目标行上 1/4（before）、1=下 1/4（after）、2=中间（inside）。
/// `target` 可为会话 id，也可为分组节点 id（`group:A/B`）。
/// 参考口径：inside 分组 → 入该组；会话行 → 入该会话所在组；
/// **分组行的 before/after → 升到该分组的父组**（`groupPath.slice(0, -1)`）。
pub fn drag_move(dragged: &str, target: &str, pos: u8) -> Result<(), String> {
    let mut all = store::list_all();
    let at = all
        .iter()
        .position(|c| c.id == dragged)
        .ok_or("拖拽源会话不存在")?;
    let mut moved = all.remove(at);
    let norm = |raw: &str| store::group_segments(raw).join(" > ");
    let group = match target.strip_prefix("group:") {
        Some(rest) => {
            if rest == "收藏的" {
                return Err("「收藏的」是虚拟分组，不作为拖放目标".to_string());
            }
            let mut segs = id_segments(rest);
            if pos != 2 && !segs.is_empty() {
                segs.pop(); // 分组行的前/后 = 升到父组
            }
            segs.join(" > ")
        }
        None => {
            let t = all
                .iter()
                .find(|c| c.id == target)
                .ok_or("目标会话不存在")?;
            t.meta.group_path.clone()
        }
    };
    moved.meta.group_path = group.clone();
    let mut idx = all.len();
    if pos < 2 {
        if let Some(p) = all.iter().position(|c| c.id == target) {
            idx = p + usize::from(pos == 1);
        }
    } else if let Some(p) = all.iter().rposition(|c| norm(&c.meta.group_path) == group) {
        idx = p + 1; // 入组 = 追加到该组末尾
    }
    all.insert(idx.min(all.len()), moved);
    store::rewrite_ordered(&all)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::store::{session_config::SessionKind, SessionConfig};
    // The global directory override is also used by palette_tests. Reuse its
    // process-wide fixture guard; a module-local mutex does not isolate the two.

    fn temp_store(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("tideterm-io-{tag}-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mktemp");
        store::set_store_dir(Some(dir.clone()));
        dir
    }

    fn ssh(name: &str, group: &str) -> SessionConfig {
        let mut c = SessionConfig::blank(SessionKind::Ssh);
        c.name = name.into();
        c.meta.group_path = group.into();
        if let Some(s) = c.ssh.as_mut() {
            s.host = "10.0.0.9".into();
            s.username = "root".into();
        }
        c
    }

    #[test]
    fn duplicate_makes_new_id_and_keeps_other_fields() {
        let _g = crate::rust::term_tests::input_lock_guard();
        let dir = temp_store("dup");
        let id = store::save_returning_id(&ssh("prod", "生产 > 数据库")).expect("save");
        let new_id = duplicate(&id).expect("duplicate");
        assert!(!new_id.is_empty() && new_id != id, "必须换新 id");
        let src = store::get_by_id(&id).expect("src");
        let dup = store::get_by_id(&new_id).expect("dup");
        assert_eq!(dup.name, "prod (副本)");
        assert_eq!(dup.meta.group_path, src.meta.group_path, "分组须保留");
        assert_eq!(
            dup.ssh.as_ref().unwrap().host,
            src.ssh.as_ref().unwrap().host,
            "主机须保留"
        );
        assert_eq!(store::list_all().len(), 2);
        assert!(duplicate("no-such-id").is_err());
        store::set_store_dir(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drag_move_regroups_reorders_and_refuses_virtual_group() {
        let _g = crate::rust::term_tests::input_lock_guard();
        let dir = temp_store("drag");
        let a = store::save_returning_id(&ssh("a", "生产")).expect("a");
        let b = store::save_returning_id(&ssh("b", "生产 > 数据库")).expect("b");
        let c = store::save_returning_id(&ssh("c", "")).expect("c");
        // ① 拖到分组行「生产」的中间 = 入组（参考 position=inside + type=group）。
        drag_move(&c, "group:生产", 2).expect("into 生产");
        assert_eq!(store::get_by_id(&c).expect("c").meta.group_path, "生产");
        // ② 拖到子分组行的上/下半 = 升到**父组**（groupPath.slice(0, -1)）。
        drag_move(&b, "group:生产/数据库", 0).expect("to parent");
        assert_eq!(store::get_by_id(&b).expect("b").meta.group_path, "生产");
        // ③ 拖到会话行上半 = before 该会话（store 序即树序）。
        drag_move(&b, &a, 0).expect("before a");
        let names: Vec<String> = store::list_all().iter().map(|x| x.name.clone()).collect();
        let pos = |n: &str| names.iter().position(|x| x == n).expect("在列表内");
        assert!(pos("b") < pos("a"), "b 须排到 a 前（store 序 = 树序）");
        // ④ 「收藏的」是虚拟分组，不接受拖放；未知目标报错不写盘。
        assert!(drag_move(&b, "group:收藏的", 2).is_err());
        assert!(drag_move(&b, "no-such-target", 0).is_err());
        assert!(drag_move("no-such-source", "group:生产", 2).is_err());
        assert_eq!(store::list_all().len(), 3, "失败路径不得增减条目");
        let _ = b;
        store::set_store_dir(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_then_import_round_trips_without_id_clash() {
        let _g = crate::rust::term_tests::input_lock_guard();
        let dir = temp_store("io");
        let id = store::save_returning_id(&ssh("prod", "生产")).expect("save");
        let file = dir.join("out.json");
        export(&id, &file.to_string_lossy()).expect("export");
        let text = std::fs::read_to_string(&file).expect("read");
        assert!(text.contains("\"prod\""), "导出须含名称");
        assert!(!text.contains("\"password\":"), "导出不得含 password 键");
        assert!(
            !text.contains("\"passphrase\":"),
            "导出不得含 passphrase 键"
        );
        // 说明：`"type": "password"` 会出现，那是 SshAuth 的 serde tag（认证方式名），
        // 不是密钥值——core 用 skip_serializing 保证值不落盘（config.rs:89-94）。
        // 回导：重生成 id ⇒ 既有条目不被覆盖，总数 +1
        assert_eq!(import(&file.to_string_lossy()).expect("import"), 1);
        assert_eq!(store::list_all().len(), 2);
        assert_eq!(store::get_by_id(&id).expect("原样").name, "prod");
        // 坏文件 → 报错（参考 importError 口径）
        let bad = dir.join("bad.json");
        std::fs::write(&bad, "{ not an array }").expect("write");
        assert!(import(&bad.to_string_lossy()).is_err());
        // 空数组 → 0 条（参考 importEmpty 口径）
        let empty = dir.join("empty.json");
        std::fs::write(&empty, "[]").expect("write");
        assert_eq!(import(&empty.to_string_lossy()).expect("empty"), 0);
        store::set_store_dir(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
