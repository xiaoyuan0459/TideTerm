//! `tiderm-textdoc` —— 「终端滚动历史」与「日志文件」统一的存储 / 索引 / 压缩层
//! （[ADR-013](../../docs/adr/ADR-013-logstore-and-compression.md)）。
//!
//! 三面分层中的**存储 / 索引面**：上承 C++ 壳（只经 cxx 值类型桥访问 Rust），
//! 下接终端引擎（`tiderm-terminal` 以 `default-features = false` 依赖
//! [`core`]，只吃 `RowProvider` / `RowRef` / `LineRange` 三个符号 —— G4-12 门禁）。
//!
//! ## 模块图
//!
//! ```text
//! core.rs      零 IO 叶子：RowProvider / RowRef / LineRange（始终编译）
//! bytesource.rs  ByteSource：本地文件 / 内存切片（SFTP 流、会话输出后续接入）
//! index.rs       LineIndex：稀疏行索引（每 256 行采样，可重扫重建、可驱逐）
//! chunk.rs       ChunkCache：已物化块 LRU（可驱逐类「日志解码缓存」）
//! tier.rs        冷层块压缩（zstd-1；D4 未定案）+ 分层统计
//! provider.rs    LogDocument：分块 + 稀疏索引 + 惰性读的 FileChunkProvider
//! manager.rs     BudgetPool：256 MiB 共池仲裁（D1a：类内 LRU + 类间优先级）
//! contract.rs    G4-11 Provider 一致性套件（两个实现共享同一套用例）
//! ```
//!
//! ## 访问模式裁决（ADR-013 §3）
//!
//! 日志 / 终端历史是 **append-only**：分块段 + 稀疏行索引（O(1) 追加、O(log n) 定位）。
//! **明确不用 rope / piece-table** —— 它们为「中间随机插入」付费，日志永远用不到。

#![allow(clippy::doc_markdown)]

/// G4-11 Provider 一致性套件 —— 纯契约逻辑（只用 [`core`]），**随 core 一起编译**：
/// widget 侧的 `MemRingProvider` 也要跑同一套用例。
pub mod contract;
pub mod core;

#[cfg(feature = "file")]
pub mod bytesource;
#[cfg(feature = "file")]
pub mod chunk;
#[cfg(feature = "file")]
pub mod index;
#[cfg(feature = "file")]
pub mod manager;
#[cfg(feature = "file")]
pub mod provider;

#[cfg(feature = "compress")]
pub mod tier;

// ---- 平铺再导出（高频符号） ----
pub use core::{LineRange, RowProvider, RowRef};

#[cfg(feature = "file")]
pub use bytesource::{ByteSource, FileSource, SliceSource};
#[cfg(feature = "file")]
pub use index::LineIndex;
#[cfg(feature = "file")]
pub use manager::{BudgetPool, PoolUsage, TerminalStatus};
#[cfg(feature = "file")]
pub use provider::LogDocument;

#[cfg(feature = "compress")]
pub use tier::{compress_cold, decompress_cold, COLD_BLOCK};
