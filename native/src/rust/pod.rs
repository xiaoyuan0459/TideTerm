//! pod 数据源（Phase C ③ 骨架 → ④ 起只剩活跃会话面）。
//!
//! 连接树数据源已迁 session 域（`sess_tree_rows`：sessions.json 持久配置树，
//! store.rs）；本域保留右侧「会话」列（活跃会话 + 实时状态）与横幅面包屑。

use crate::bridge::pod::pod_ffi::PodRow;
use crate::rust::term_diag;

fn row(id: &str, label: &str, kind: u8, depth: u32, protocol: &str, status: &str) -> PodRow {
    PodRow {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        depth,
        protocol: protocol.to_string(),
        status: status.to_string(),
    }
}

/// 右侧「会话」列快照：活跃会话 + 实时状态文本。
pub fn pod_sessions() -> Vec<PodRow> {
    term_diag::registry_snapshot()
        .into_iter()
        .map(|(id, label)| {
            let status = term_diag::term_status(id);
            let cfg = super::term_lifecycle::active_config(id);
            row(&id.to_string(), &label, 1, 0, cfg.protocol_str(), &status)
        })
        .collect()
}

/// 会话面包屑：`本地 › <标题>`（活跃会话视图；持久配置树的路径在连接树侧）。
pub fn pod_path(idx: u32) -> String {
    let title = term_diag::term_title(idx);
    format!("本地 › {title}")
}
