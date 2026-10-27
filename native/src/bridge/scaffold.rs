//! 桥拆域 spike（ADR-014 §Decision 3）—— 最小独立域桥。
//!
//! 验证机制：cxx 生成代码对 `extern "Rust"` 实现的解析走 `super::` 相对路径，
//! 桥挂文件模块时 `super::` = 本文件（`crate::bridge::scaffold`）——因此实现符号
//! 必须在**本文件作用域可见**。下方顶层 `pub use` 把实现从 `rust::spike` 重导出
//! 到此（生产形态：域实现分散在 rust/ 各模块，桥文件只做声明 + 重导出）。
//! 主桥内联 main.rs 的根级 `pub use` 是同一机制在 crate 根的等价物。

pub use crate::rust::spike::spike_rust_id;

#[cxx::bridge]
pub mod scaffold_ffi {
    extern "Rust" {
        /// Rust 侧域常量（cxx 声明在 C++ 全局作用域——跨桥函数名须全局唯一）。
        fn spike_rust_id() -> u32;
    }

    unsafe extern "C++" {
        // 各域各生成头：本桥的声明在 native/src/bridge/scaffold.rs.h。
        include!("cpp/bridge_spike.h");
        /// C++ 侧透传：返回 spike_rust_id() + v（Rust→C++→Rust 往返断言用）。
        fn qt_spike_passthrough(v: u32) -> u32;
    }
}
