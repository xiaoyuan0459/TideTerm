//! 桥拆域 spike 的实现符号（ADR-014 §Decision 3）。
//!
//! 本模块模拟生产形态：实现落在**非桥文件**（rust/ 下），由桥文件
//! `bridge/scaffold.rs` 顶层 `pub use` 重导出到桥父模块作用域，供 cxx 生成代码
//! 的 `super::` 相对路径解析——这是「桥必须内联 main.rs」硬规则（E0425 实测）
//! 的解锁条件，也是本 spike 要验证的唯一机制。

/// Rust 侧域常量（自检期望值 = 0x5D1E + 入参）。
pub fn spike_rust_id() -> u32 {
    0x5D1E
}
