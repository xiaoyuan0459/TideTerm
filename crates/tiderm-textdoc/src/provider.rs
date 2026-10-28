//! `LogDocument` —— FileChunkProvider（D2 第 1 顺位：本地文件）。
//!
//! 分块（64 KiB）+ 稀疏行索引 + 惰性读：**打开不整读**（G4-1）——首屏只
//! 索引前 [`FIRST_INDEX_WINDOW`] 字节并物化 1–2 个块；全量索引由
//! [`LogDocument::index_more`] 分步推进（宿主后台线程驱动）。
//!
//! 内存构成（预算池记账口径）：
//! * 解码缓存 = [`ChunkCache`](crate::chunk::ChunkCache)（字节上限 LRU，可驱逐）；
//! * 索引 = [`LineIndex`](crate::index::LineIndex)（采样表，可驱逐、可重扫重建）；
//! * 冷层 = **磁盘本身**（按需 pread，不驻留 RAM —— G4-6 的机制来源）；
//! * 温/热层 = 0（文件类不持有 RAM 行表；终端类的温层在 `ScrollbackStore`）。
//!
//! 行提取：定位起点所在块（命中解码缓存或 pread），跳行/找行尾跨块时把后续块
//! **增量拼接**进一次性的工作缓冲；行内容 memcpy 进 `line_buf`（平均百字节级，
//! 可忽略于 pread）——`RowRef` 有效期「至下一次 `row()`」，与 trait 契约一致。

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::bytesource::{ByteSource, FileSource};
use crate::chunk::{ChunkCache, CHUNK_SIZE};
use crate::core::{RowProvider, RowRef};
use crate::index::LineIndex;

/// 首屏同步索引窗口：打开时先索引这么多字节（2 MiB 顺序扫描是毫秒级，
/// G4-2 ≤500 ms 的余量极大）。
pub const FIRST_INDEX_WINDOW: u64 = 2 * 1024 * 1024;

/// 索引步进上限（`index_more` 单次调用内部的最大单块读）。
pub const INDEX_STEP: u64 = CHUNK_SIZE;

/// 跨块长行的拼装上限（超过按当前缓冲截断 —— 防御性常量，正常日志远达不到）。
const SPILL_CAP: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocState {
    Indexing,
    Ready,
}

pub struct LogDocument {
    source: Box<dyn ByteSource>,
    index: LineIndex,
    cache: RefCell<ChunkCache>,
    /// 每次物化/扫描读取的字节计数（累计）——探针用它核对「不整读」（G4-1）。
    pub io_bytes: std::sync::atomic::AtomicU64,
}

impl LogDocument {
    /// 打开本地文件并惰性索引（首屏路径：只索引前 `FIRST_INDEX_WINDOW` 字节）。
    pub fn open(path: impl AsRef<Path>) -> io::Result<LogDocument> {
        let source = FileSource::open(path)?;
        Ok(Self::from_source(Box::new(source)))
    }

    /// 从任意 `ByteSource` 构造（测试 / SFTP 流 / 会话捕获的入口形状）。
    pub fn from_source(source: Box<dyn ByteSource>) -> LogDocument {
        let mut doc = LogDocument {
            source,
            index: LineIndex::new(),
            cache: RefCell::new(ChunkCache::new(8 * CHUNK_SIZE as usize)),
            io_bytes: std::sync::atomic::AtomicU64::new(0),
        };
        doc.index_more(FIRST_INDEX_WINDOW);
        doc
    }

    /// 后台索引驱动：最多推进 `max_bytes` 字节，返回实际推进量；完成（EOF）返回 0。
    pub fn index_more(&mut self, max_bytes: u64) -> u64 {
        let mut done: u64 = 0;
        while done < max_bytes && !self.index.complete() {
            let off = self.index.bytes_indexed();
            let mut buf = vec![0u8; INDEX_STEP as usize];
            let n = match self.source.read_at(off, &mut buf) {
                Ok(n) => n,
                Err(_) => return done,
            };
            let is_last = (n as u64) < buf.len() as u64; // 短读 = EOF
            buf.truncate(n);
            self.index.feed(&buf, is_last);
            done += n as u64;
        }
        done
    }

    pub fn state(&self) -> DocState {
        if self.index.complete() {
            DocState::Ready
        } else {
            DocState::Indexing
        }
    }

    /// 源长度（字节；来自 ByteSource 的长度提示）。
    pub fn source_len(&self) -> Option<u64> {
        self.source.len_hint()
    }

    pub fn indexed_lines(&self) -> u64 {
        self.index.indexed_lines()
    }

    /// 预算池记账：`(解码缓存, 索引)` 字节。
    pub fn ram_bytes(&self) -> (usize, usize) {
        (self.cache.borrow().bytes(), self.index.memory_bytes())
    }

    /// D1a 驱逐第 3 项：丢弃解码缓存。返回释放字节。
    pub fn evict_decode(&mut self) -> usize {
        self.cache.borrow_mut().evict_all()
    }

    /// D1a 驱逐第 4 项：丢弃索引采样表（可重扫重建）。返回释放字节。
    /// 驱逐后 `row()` 正确性不变（定位退化为从 0 扫描），仅变慢 ——
    /// 宿主应在空闲时 [`LogDocument::reindex`]。
    pub fn evict_index(&mut self) -> usize {
        self.index.evict()
    }

    /// 重建索引（驱逐后的后台动作）：从头重扫源。
    pub fn reindex(&mut self) {
        self.index = LineIndex::new();
        self.index_more(u64::MAX);
    }

    fn load_chunk(&self, base: u64) -> Arc<Vec<u8>> {
        let mut cache = self.cache.borrow_mut();
        if let Some(d) = cache.get(ChunkCache::chunk_id(base)) {
            return d;
        }
        drop(cache);
        let mut data = vec![0u8; CHUNK_SIZE as usize];
        let mut filled = 0usize;
        while filled < data.len() {
            let n = self
                .source
                .read_at(base + filled as u64, &mut data[filled..])
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            filled += n;
        }
        data.truncate(filled);
        let arc = Arc::new(data);
        self.cache
            .borrow_mut()
            .insert(ChunkCache::chunk_id(base), arc.clone());
        arc
    }

    /// 行提取主路径：定位 → 跳 `skip` 个换行 → 找行尾，跨块时增量拼接工作缓冲。
    /// 返回行内容（owned；`row()` 再归一为 `RowRef::Utf8Owned`）。
    fn extract_line(&self, line: u64) -> Option<Vec<u8>> {
        let (start_off, skip) = self.index.scan_start(line)?;
        let file_len = self.source.len_hint().unwrap_or(u64::MAX);
        let base0 = (start_off / CHUNK_SIZE) * CHUNK_SIZE;
        let mut work: Vec<u8> = self.load_chunk(base0).as_ref().clone();
        loop {
            let rel = (start_off - base0) as usize;
            // 1) 跳过 `skip` 个换行。
            let mut pos = rel.min(work.len());
            let mut left = skip;
            while pos < work.len() && left > 0 {
                if work[pos] == b'\n' {
                    left -= 1;
                }
                pos += 1;
            }
            if left > 0 {
                if base0 + work.len() as u64 >= file_len {
                    return None; // 数据短于索引（防御）
                }
                self.append_next(&mut work, base0);
                continue;
            }
            // 2) 找行尾。
            if let Some(nl) = work[pos..].iter().position(|&b| b == b'\n') {
                return Some(work[pos..pos + nl].to_vec());
            }
            // 无换行：行跨块或到 EOF 尾行。
            if base0 + work.len() as u64 >= file_len || work.len() as u64 > SPILL_CAP {
                return Some(work[pos..].to_vec());
            }
            self.append_next(&mut work, base0);
        }
    }

    /// 把下一块追加进工作缓冲。work 覆盖绝对区间 [base0, base0 + len)，
    /// 下一块的源偏移 = base0 + work.len()。
    fn append_next(&self, work: &mut Vec<u8>, base0: u64) {
        let next_off = base0 + work.len() as u64;
        let mut chunk = vec![0u8; CHUNK_SIZE as usize];
        let mut filled = 0usize;
        while filled < chunk.len() {
            let n = self
                .source
                .read_at(next_off + filled as u64, &mut chunk[filled..])
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            filled += n;
        }
        chunk.truncate(filled);
        work.extend_from_slice(&chunk);
    }
}

impl RowProvider for LogDocument {
    fn rows(&self) -> usize {
        self.index.indexed_lines() as usize
    }

    fn row(&self, i: usize) -> Option<RowRef<'_>> {
        let line = self.extract_line(i as u64)?;
        Some(RowRef::Utf8Owned(line.into()))
    }
}
