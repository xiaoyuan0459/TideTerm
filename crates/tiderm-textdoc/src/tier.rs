//! 冷层块压缩（ADR-013 §5「热 / 温 / 冷」的冷层实现件）。
//!
//! **D4 未定案的实测输入**：本 POC 用 **zstd-1**（离线 cargo 缓存有
//! zstd-sys 2.0.16；lz4 不在缓存中）。换 lz4 只动本文件；「冷层块级压缩 +
//! 64 KiB 块粒度」的结构决策不受影响。
//!
//! 作用面：**终端滚动历史的冷端**（`ScrollbackStore` 温层之后）与
//! **流捕获的累积块**（StreamProvider）。日志文件的「冷层」是磁盘本身
//! （FileChunkProvider 按需 pread，根本不驻留 RAM）—— 这正是 G4-6
//! 「内存不随文件大小线性增长」的机制来源。

/// 冷层块粒度：64 KiB（与 [`crate::chunk::CHUNK_SIZE`] 一致）。
pub const COLD_BLOCK: usize = 64 * 1024;

/// 压缩一个冷层块（zstd level 1 —— LZ4 档位的速度级别、更高压缩率）。
pub fn compress_cold(raw: &[u8]) -> std::io::Result<Vec<u8>> {
    zstd::bulk::compress(raw, 1)
}

/// 解压一个冷层块。`hint_cap` 为原始大小提示（解压输出上界，低估会报错）。
pub fn decompress_cold(raw: &[u8], hint_cap: usize) -> std::io::Result<Vec<u8>> {
    zstd::bulk::decompress(raw, hint_cap)
}

/// 分层用量报表（G4-7 / 可观测性口径）。
#[derive(Debug, Default, Clone, Copy)]
pub struct TierStats {
    /// 热层（视口 ±N 行，未压缩）。
    pub hot_bytes: usize,
    /// 温层（RLE + interner + 空白共享 —— 现有 ScrollbackStore 能力）。
    pub warm_bytes: usize,
    /// 冷层（块级压缩后）。
    pub cold_bytes: usize,
    /// 冷层解压前的原始字节数（压缩比分母）。
    pub cold_raw_bytes: usize,
}

impl TierStats {
    pub fn total(&self) -> usize {
        self.hot_bytes + self.warm_bytes + self.cold_bytes
    }

    /// 冷层压缩比（≥1.0；无冷数据时返回 0）。
    pub fn cold_ratio(&self) -> f64 {
        if self.cold_bytes == 0 {
            0.0
        } else {
            self.cold_raw_bytes as f64 / self.cold_bytes as f64
        }
    }
}
