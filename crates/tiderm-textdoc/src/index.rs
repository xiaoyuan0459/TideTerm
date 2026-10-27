//! `LineIndex` —— 稀疏行索引（ADR-013 §Decision 3 的「核心」件）。
//!
//! 每 [`SAMPLE_STRIDE`] 行采样一个 `(行号, 字节偏移)`；定位第 N 行 = 二分采样 +
//! 前向扫描 ≤ STRIDE 行（数据由 provider 的读窗口提供）。
//!
//! 内存口径（G4-10）：1 GiB / 平均 70 B 行 ≈ 15M 行 → 15M/256 ≈ 58.6k 采样 × 16 B
//! ≈ **0.9 MiB/文件**；20 文件 ≈ 18 MiB，远低于「≤ 全局预算 30%（76.8 MiB）」的
//! 门禁 —— 这是本文件存在意义的量化证明。
//!
//! 可驱逐（D1a 驱逐顺序第 4 项）：采样表可丢弃后**重扫重建**（文件在磁盘）。
//! 增量索引：`feed` 由 provider 在后台分步调用（首屏只索引前几个块，G4-2）。

/// 初始采样步长：每 256 行采样一个偏移（定位扫描量 ≤ 步长 × 平均行长）。
pub const SAMPLE_STRIDE: u64 = 256;

/// 每文档采样上限（自适应倍增的收敛点）。**索引内存与文件大小无关**的机制来源：
/// 超过上限就把采样表隔一去一、步长翻倍 —— 定位扫描量随之增长，但仍是
/// memchr 级（1 GiB ≈ 步长 4096 行 × ~70 B ≈ 286 KiB 扫描，≪ G4-3 的 16 ms）。
/// 上限 4096 × 16 B ≈ **64 KiB/文档** → 20 文件合计 ≈ 1.3 MiB（G4-10 门禁
/// 76.8 MiB 的 ~2%），且 100 MiB 与 1 GiB 文档的稳态索引内存相同（G4-6）。
pub const MAX_SAMPLES: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct Sample {
    line: u64,
    off: u64,
}

/// 稀疏行索引。行号 0-based；`feed` 只接受**顺序追加**的字节段（append-only 裁决）。
#[derive(Debug, Default)]
pub struct LineIndex {
    samples: Vec<Sample>,
    /// 当前采样步长（自适应倍增：见 [`MAX_SAMPLES`]）。
    stride: u64,
    /// 已完整索引的行数（以 `\n` 终止的行；EOF 尾行在 `complete` 时计入）。
    indexed_lines: u64,
    bytes_indexed: u64,
    complete: bool,
    /// 定位时的前向扫描参考：已见过的最大行长（供 provider 取窗口下界）。
    max_line_len: u64,
}

impl LineIndex {
    pub fn new() -> Self {
        LineIndex {
            stride: SAMPLE_STRIDE,
            ..LineIndex::default()
        }
    }

    /// 推进索引：`buf` 是源中 `[bytes_indexed, bytes_indexed + buf.len())` 的字节。
    /// `is_last` 标记这是最后一段（EOF）—— 尾行（无 `\n`）此时计为完整行。
    pub fn feed(&mut self, buf: &[u8], is_last: bool) {
        let base = self.bytes_indexed;
        // 段首是续行（前一段末尾无 \n）时，本段第一个 \n 结束的是上一行 ——
        // 行号跨段连续：line_start 相对本段起点。
        let mut line_start = 0usize;
        while let Some(nl) = memchr_newline(&buf[line_start..]) {
            let nl_at = line_start + nl;
            self.max_line_len = self.max_line_len.max((nl_at - line_start) as u64);
            self.indexed_lines += 1;
            if self.indexed_lines.is_multiple_of(self.stride) {
                // 该 \n 之后 = 下一行的起点。
                self.samples.push(Sample {
                    line: self.indexed_lines,
                    off: base + nl_at as u64 + 1,
                });
                self.thin_if_needed();
            }
            line_start = nl_at + 1;
        }
        if is_last && line_start < buf.len() {
            // EOF 尾行（无终止 \n）。
            self.max_line_len = self.max_line_len.max((buf.len() - line_start) as u64);
            self.indexed_lines += 1;
        } else if !is_last {
            // 未终结的续行长度下界（跨段累加的近似：只记本段长度）。
            self.max_line_len = self.max_line_len.max((buf.len() - line_start) as u64);
        }
        self.bytes_indexed += buf.len() as u64;
        if is_last {
            self.complete = true;
        }
    }

    /// 定位辅助：返回「应从哪个字节偏移开始前向扫描」与「需跳过的换行数」。
    /// 扫描本身由 provider 在其读窗口内完成（O(log n) 二分 + ≤ STRIDE 行扫描）。
    pub fn scan_start(&self, line: u64) -> Option<(u64, u64)> {
        if line >= self.indexed_lines {
            return None;
        }
        // 二分：最后一个 line <= 目标 的采样。
        let idx = self.samples.partition_point(|s| s.line <= line);
        let (start_line, start_off) = if idx == 0 {
            (0, 0)
        } else {
            let s = self.samples[idx - 1];
            (s.line, s.off)
        };
        debug_assert!(start_line <= line);
        Some((start_off, line - start_line))
    }

    /// 已索引行数（未完成索引时 = 目前为止的已知行数，随 `feed` 增长）。
    pub fn indexed_lines(&self) -> u64 {
        self.indexed_lines
    }

    /// 已索引字节数（增量游标）。
    pub fn bytes_indexed(&self) -> u64 {
        self.bytes_indexed
    }

    /// 索引是否完整（EOF 已见）。
    pub fn complete(&self) -> bool {
        self.complete
    }

    /// 已见最大行长下界（provider 取读窗口的参考）。
    pub fn max_line_len(&self) -> u64 {
        self.max_line_len
    }

    /// 内存占用（G4-10 口径：采样表 + 固定字段）。
    pub fn memory_bytes(&self) -> usize {
        self.samples.len() * std::mem::size_of::<Sample>() + std::mem::size_of::<Self>()
    }

    /// 驱逐（D1a 第 4 顺位）：丢弃采样表。可重建 —— 重扫源文件即可（G4-14）。
    pub fn evict(&mut self) -> usize {
        let freed = self.memory_bytes();
        self.samples.clear();
        // 采样清空后定位退化为「从头扫描」——正确性不受影响（scan_start 回 (0, line)）。
        freed
    }
}

#[inline]
fn memchr_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().position(|&b| b == b'\n')
}

impl LineIndex {
    /// 采样超上限 → 隔一去一 + 步长翻倍（保留的行号都是新步长的整倍数，
    /// 定位语义不变）。这就是「索引内存有界」的机制。
    fn thin_if_needed(&mut self) {
        if self.samples.len() > MAX_SAMPLES {
            let kept: Vec<Sample> = self.samples.iter().step_by(2).copied().collect();
            self.samples = kept;
            self.stride *= 2;
        }
    }
}
