//! pod 域桥（ADR-014 §Decision 3「按域拆桥」第二域，Phase C ③）。
//!
//! 面向 chrome 的**活跃会话**快照：右侧会话列、横幅面包屑。连接树数据源
//! Phase C ④ 起迁 session 域（`sess_tree_rows`）。桥面铁律同主桥：只传 POD。
//! 实现符号的顶层 `pub use` 先于桥声明；`pod_` 前缀跨桥全局唯一。

pub use crate::rust::pod::{pod_path, pod_sessions};

#[cxx::bridge]
pub mod pod_ffi {
    /// chrome 会话快照行（会话列/面包屑共用；全部 POD 字段）。
    pub struct PodRow {
        /// 会话稳定 id（与 term_* 的 idx 同源）；分组用 `group:<路径>`。
        pub id: String,
        /// 显示名（会话 OSC 标题回退「终端 N」）。
        pub label: String,
        /// 0=分组 1=会话。
        pub kind: u8,
        /// 嵌套深度（会话列平铺 = 0）。
        pub depth: u32,
        /// 协议名（local/ssh/…）。
        pub protocol: String,
        /// 状态文本（右侧会话列用）。
        pub status: String,
    }

    extern "Rust" {
        /// 右侧「会话」列快照：当前全部活跃会话（按创建序）。
        fn pod_sessions() -> Vec<PodRow>;
        /// 会话 idx 的面包屑路径（` › ` 连接，如「本地 › 终端 1」）。
        fn pod_path(idx: u32) -> String;
    }
}
