//! `ByteSource` —— 数据来源抽象（ADR-013 §Decision 4）。
//!
//! 用「字节源」而不是「路径」建模，因为日志有三个来源：**本地文件**（G4-A 即用）、
//! **会话输出实时捕获**（替换 `session_log.rs`）、**SFTP 远程流**（复用 v1 Phase C
//! 里程碑 5 通路）。三者共用同一套索引与压缩，差异只在实现。
//!
//! 与 [`crate::core::RowProvider`]（不带 Send）不同，本 trait **要求 `Send`**：
//! 它是后台索引 / 压缩 worker 的入口（trait 形状定稿记录见 core.rs 模块文档）。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

/// 字节源：随机读 + 长度提示（流式来源 `len_hint` 返回 `None`）。
pub trait ByteSource: Send {
    /// 从 `off` 起读至多 `buf.len()` 字节，返回实际读取数（0 = EOF）。
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// 源长度（字节）；流式来源为 `None`。
    fn len_hint(&self) -> Option<u64>;
}

/// 本地文件源（D2 第 1 顺位）。`pread` 语义：std 无 positional read，用
/// `Mutex<File>` + seek/read 串行化（调用方是单线程 UI + 单线程索引 worker，
/// 锁竞争可忽略）。
pub struct FileSource {
    file: Mutex<File>,
    len: u64,
    /// 诊断计数：实际从磁盘读过的字节（G4-1「首屏不整读」的判定依据）。
    pub bytes_read: std::sync::atomic::AtomicU64,
}

impl FileSource {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(FileSource {
            file: Mutex::new(file),
            len,
            bytes_read: std::sync::atomic::AtomicU64::new(0),
        })
    }
}

impl ByteSource for FileSource {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || off >= self.len {
            return Ok(0);
        }
        let mut f = self.file.lock().expect("file source poisoned");
        f.seek(SeekFrom::Start(off))?;
        let n = f.read(buf)?;
        self.bytes_read
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }

    fn len_hint(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// 内存切片源（测试 / 已整读的小文件）。
pub struct SliceSource {
    data: Vec<u8>,
}

impl SliceSource {
    pub fn new(data: Vec<u8>) -> Self {
        SliceSource { data }
    }
}

impl ByteSource for SliceSource {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        let off = off as usize;
        if off >= self.data.len() || buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }

    fn len_hint(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}
