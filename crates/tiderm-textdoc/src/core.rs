//! 零 IO 叶子模块 —— `RowProvider` 契约（ADR-013 §Decision 2）。
//!
//! 本模块是整个 crate 的依赖根：不依赖任何 IO / 压缩设施，因此
//! `tiderm-terminal` 可以 `default-features = false` 地依赖本 crate 而
//! 保持日志文件/压缩依赖与终端独立工作区隔离（G4-12 硬门禁）。
//!
//! ## 契约（G4-11 一致性套件就是按这些断言写的）
//!
//! * `rows()` 返回当前可读行数；`row(i)` 对 `i >= rows()` 必须返回 `None`。
//! * `row(i)` 的内容用 [`RowRef::write_utf8`] 归一后必须**逐字节稳定**（同一 provider
//!   反复读同一行、驱逐后重读，内容不变 —— G4-14）。
//! * [`RowProvider::ranges`] 是折叠 / 分组区间，默认无（空表）。
//!
//! ## trait 形状定稿记录（G4-A 的交付物之一）
//!
//! * **`RowProvider` 不带 `Send`**。v2 计划 §4.3 草图里的 `Send` 与
//!   `ScrollbackStore` 的 `Rc<[u16]>` 内部表示冲突（温层行是 UI 线程对象）。
//!   并发要求落在该在的层：[`crate::bytesource::ByteSource`]（索引 / 压缩 worker
//!   的入口）要求 `Send`；行访问始终发生在拥有 provider 的线程上。
//! * `RowRef` 增加 `Utf16(&[u16])` 变体：终端行的每列一个 UTF-16 码元
//!   （`ScrollbackStore::LineData.chars`）可以**零拷贝**借出，不必升格 Arc。
//!   `Shared(Arc<[u16]>)` 留给跨视图共享的已提交行（StreamProvider / 未来 Arc 化的
//!   interner）。

use std::sync::Arc;

/// 折叠 / 分组区间：行号 `[start, end)`（半开区间，`end` 是 exclusive）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// 一行的内容。变体按来源选择（详见模块文档）：
/// * `Utf8` —— 日志 / 文件来源，借用自 provider 内的稳定缓冲；
/// * `Utf8Owned` —— 同上但行缓存带内变（解码窗口 / LRU），借不出 `&self` 生命周期，
///   以 owned Arc 归一（FileChunkProvider 路径；一次 ≤ 行长的引用计数分配）；
/// * `Utf16` —— 终端列行，借用自温层行存储（每列一个码元）；
/// * `Shared` —— 跨视图共享的已提交行（Arc，引用计数 bump）；
/// * `Blank` —— 全空白行（终端最常见行；宽度由消费方按布局补齐）。
#[derive(Debug, Clone)]
pub enum RowRef<'a> {
    Utf8(&'a [u8]),
    Utf8Owned(Arc<[u8]>),
    Utf16(&'a [u16]),
    Shared(Arc<[u16]>),
    Blank,
}

impl RowRef<'_> {
    /// 归一为 UTF-8 字节 —— 一致性契约与测试的比较口径。
    /// `Utf16` 按 UTF-16 语义解码（代理对合成；孤立代理替换为 U+FFFD）。
    pub fn write_utf8(&self, out: &mut Vec<u8>) {
        match self {
            RowRef::Utf8(b) => out.extend_from_slice(b),
            RowRef::Utf8Owned(b) => out.extend_from_slice(b),
            RowRef::Blank => {}
            RowRef::Utf16(u) => write_utf16(u, out),
            RowRef::Shared(u) => write_utf16(u.as_ref(), out),
        }
    }
}

/// UTF-16 码元序列 → UTF-8（代理对合成，孤立代理 → U+FFFD）。
fn write_utf16(units: &[u16], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        let ch = if (0xD800..0xDC00).contains(&u) {
            if let Some(&lo) = units.get(i + 1) {
                if (0xDC00..0xE000).contains(&lo) {
                    i += 1;
                    let c = 0x10000 + ((u as u32 - 0xD800) << 10) + (lo as u32 - 0xDC00);
                    char::from_u32(c)
                } else {
                    None
                }
            } else {
                None
            }
        } else if (0xDC00..0xE000).contains(&u) {
            None
        } else {
            char::from_u32(u as u32)
        };
        match ch {
            Some(c) => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            None => out.extend_from_slice("\u{FFFD}".as_bytes()),
        }
        i += 1;
    }
}

/// 统一的行供给契约：终端滚动历史（`MemRingProvider`）与日志文件
/// （`FileChunkProvider`）/ 流捕获（`StreamProvider`，后续）是同一契约的实现。
///
/// 访问模式（ADR-013 §3，选型裁决依据）：**只追加**、按行定位、按视口取窗口。
pub trait RowProvider {
    /// 当前可读行数。流式来源随追加 / 索引推进增长。
    fn rows(&self) -> usize;

    /// 第 `i` 行（0-based）。`i >= rows()` 时返回 `None`。
    /// 返回值借用自 provider 内部缓存：**有效期至下一次 `row()` 调用**
    /// （视口消费是即时的，缓存可能在下次调用时被驱逐 / 重载 —— G4-14 保证重读一致）。
    fn row(&self, i: usize) -> Option<RowRef<'_>>;

    /// 折叠 / 分组区间（日志专属能力）。默认无区间。
    fn ranges(&self) -> &[LineRange] {
        &[]
    }
}
