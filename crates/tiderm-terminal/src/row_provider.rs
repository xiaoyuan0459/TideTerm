//! `MemRingProvider` —— 把现有 `ScrollbackStore`（温层实现）接入
//! [`RowProvider`](tiderm_textdoc::core::RowProvider) 契约
//! （[ADR-013](../../docs/adr/ADR-013-logstore-and-compression.md) §Decision 2，D5）。
//!
//! 载荷以 Arc 在线程和快照间共享。前缀压缩行在 RowRef 边界展开为连续 UTF-16，
//! 普通行复用已有 Arc；空白行走 RowRef::Blank，零分配。
//!
//! **已提交的历史行不在任何驱逐候选集内**（D1a 类型级保证在
//! `tiderm_textdoc::manager::EvictTarget`）—— 本适配器只读，无人能经它丢历史。

use crate::scrollback::ScrollbackStore;
use tiderm_textdoc::core::{RowProvider, RowRef};

/// 终端滚动历史的 `RowProvider` 视图（借用；`ScrollbackStore` 归 `TermGrid` 所有）。
pub struct MemRingProvider<'a> {
    store: &'a ScrollbackStore,
}

impl<'a> MemRingProvider<'a> {
    pub fn new(store: &'a ScrollbackStore) -> Self {
        MemRingProvider { store }
    }
}

impl RowProvider for MemRingProvider<'_> {
    fn rows(&self) -> usize {
        self.store.rows()
    }

    fn row(&self, i: usize) -> Option<RowRef<'_>> {
        let ld = self.store.row(i)?;
        if ld.is_blank() {
            return Some(RowRef::Blank);
        }
        Some(RowRef::Shared(ld.chars.to_arc()))
    }
}
