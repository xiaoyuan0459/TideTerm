//! `ChunkCache` —— 已物化块的字节上限 LRU（可驱逐类「日志解码缓存」）。
//!
//! 块按文件偏移对齐 [`CHUNK_SIZE`]（64 KiB）。跨块的长行由 provider 用
//! `spill` 缓冲拼装（不进缓存）。字节上限触发时按最久未用驱逐 —— 驱逐后
//! 重读即重新 pread（内容不变，G4-14 的可重建性由磁盘保证）。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// 块大小：64 KiB（ADR-013 §5 冷层块粒度；G4-7 的压缩块同粒度）。
pub const CHUNK_SIZE: u64 = 64 * 1024;

#[derive(Default)]
pub struct ChunkCache {
    cap_bytes: usize,
    bytes: usize,
    hits: u64,
    misses: u64,
    /// LRU 顺序：队尾 = 最近使用。
    order: VecDeque<u64>,
    entries: HashMap<u64, Arc<Vec<u8>>>,
}

impl ChunkCache {
    pub fn new(cap_bytes: usize) -> Self {
        ChunkCache {
            cap_bytes,
            ..ChunkCache::default()
        }
    }

    pub fn chunk_id(off: u64) -> u64 {
        off / CHUNK_SIZE
    }

    pub fn get(&mut self, id: u64) -> Option<Arc<Vec<u8>>> {
        if self.entries.contains_key(&id) {
            self.hits += 1;
            self.touch(id);
            self.entries.get(&id).cloned()
        } else {
            self.misses += 1;
            None
        }
    }

    /// 插入块（必要时先驱逐到容量内；单块本身超上限时直接放行并记账 ——
    /// 不丢刚读的数据，交给上层预算池仲裁）。
    pub fn insert(&mut self, id: u64, data: Arc<Vec<u8>>) {
        if self.entries.contains_key(&id) {
            self.touch(id);
            return;
        }
        self.bytes += data.len();
        self.order.push_back(id);
        self.entries.insert(id, data);
        while self.bytes > self.cap_bytes && self.order.len() > 1 {
            let victim = self.order.pop_front().expect("order non-empty");
            if let Some(d) = self.entries.remove(&victim) {
                self.bytes -= d.len();
            }
        }
    }

    fn touch(&mut self, id: u64) {
        if let Some(pos) = self.order.iter().position(|&x| x == id) {
            self.order.remove(pos);
            self.order.push_back(id);
        }
    }

    /// 驱逐全部（D1a 驱逐顺序第 3 项「日志解码缓存」）。返回释放的字节数。
    pub fn evict_all(&mut self) -> usize {
        let freed = self.bytes;
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
        freed
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}
