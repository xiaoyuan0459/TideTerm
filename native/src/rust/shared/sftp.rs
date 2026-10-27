//! Self-built SFTP browser + transfer model (T2.5) — mirrors the backend SFTP types
//! (`src/tTauri/src/protocol/sftp.rs`) and the frontend `src/types/sftp.ts`.
//!
//! The native shell owns its own model; the backend (B4) maps these to the `sftp_*`
//! public API when it lands. Today the SessionModel binding drives the browser UI.
#![allow(dead_code)] // transfer/mutation helpers are exercised once the B4 backend lands

/// A directory entry from `list_dir`.
#[derive(Debug, Clone)]
pub struct SftpFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_unix: i64,
    pub permissions: u32,
    pub owner: Option<String>,
    pub group: Option<String>,
}

impl SftpFileEntry {
    /// Human-readable size (B/KB/MB/GB) or `--` for directories.
    pub fn size_label(&self) -> String {
        if self.is_dir {
            return "--".to_string();
        }
        fmt_size(self.size_bytes)
    }

    /// Human-readable POSIX permission string (e.g. `drwxr-xr-x`).
    pub fn perm_label(&self) -> String {
        let mut s = String::new();
        s.push(if self.is_dir { 'd' } else { '-' });
        let bits = [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1];
        let chars = ['r', 'w', 'x'];
        for (i, b) in bits.iter().enumerate() {
            s.push(if self.permissions & b != 0 {
                chars[i % 3]
            } else {
                '-'
            });
        }
        s
    }
}

/// Transfer direction for progress display.
#[derive(Debug, Clone)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// A queued transfer (upload or download) with progress.
#[derive(Debug, Clone)]
pub struct TransferItem {
    pub id: String,
    pub session_id: String,
    pub file_name: String,
    pub direction: TransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub completed: bool,
    /// Human-readable error message when the transfer failed.
    pub error: Option<String>,
    /// Local source/target path used for retry.
    pub local_path: String,
    /// Remote source/target path used for retry.
    pub remote_path: String,
}

impl TransferItem {
    pub fn progress_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.bytes_transferred as f32 / self.total_bytes as f32).clamp(0.0, 1.0)
        }
    }

    pub fn direction_label(&self) -> &'static str {
        match self.direction {
            TransferDirection::Upload => "上传",
            TransferDirection::Download => "下载",
        }
    }
}

/// Format a byte count into a human-readable label.
pub fn fmt_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Sort entries: directories first, then by key (`name` | `size` | `time`).
/// Shared by the SFTP browser's two panes (Phase B).
pub(crate) fn sort_entries(list: &mut [SftpFileEntry], key: &str) {
    list.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| match key {
            "size" => b.size_bytes.cmp(&a.size_bytes),
            "time" => b.modified_unix.cmp(&a.modified_unix),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: bool, size: u64, mode: u32) -> SftpFileEntry {
        SftpFileEntry {
            name: "x".into(),
            path: "/x".into(),
            is_dir: dir,
            size_bytes: size,
            modified_unix: 0,
            permissions: mode,
            owner: None,
            group: None,
        }
    }

    #[test]
    fn dir_size_is_sentinel() {
        assert_eq!(entry(true, 0, 0o755).size_label(), "--");
    }

    #[test]
    fn file_size_formats_units() {
        assert_eq!(entry(false, 500, 0o644).size_label(), "500 B");
        assert_eq!(entry(false, 2048, 0o644).size_label(), "2.0 KB");
    }

    #[test]
    fn perm_label_renders_dirs_and_files() {
        assert_eq!(entry(true, 0, 0o755).perm_label(), "drwxr-xr-x");
        assert_eq!(entry(false, 0, 0o644).perm_label(), "-rw-r--r--");
    }

    #[test]
    fn transfer_progress_clamps() {
        let t = TransferItem {
            id: "t".into(),
            session_id: "s".into(),
            file_name: "f".into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 50,
            total_bytes: 100,
            completed: false,
            error: None,
            local_path: "local".into(),
            remote_path: "remote".into(),
        };
        assert!((t.progress_fraction() - 0.5).abs() < 1e-6);
        assert_eq!(t.direction_label(), "上传");
    }

    #[test]
    fn file_size_formats_large_units() {
        assert_eq!(entry(false, 5 * 1024 * 1024, 0).size_label(), "5.0 MB");
        assert_eq!(
            entry(false, 2 * 1024 * 1024 * 1024, 0).size_label(),
            "2.0 GB"
        );
    }

    #[test]
    fn transfer_progress_zero_total_is_zero_and_clamps_above_one() {
        let zero = TransferItem {
            id: "z".into(),
            session_id: "s".into(),
            file_name: "z".into(),
            direction: TransferDirection::Download,
            bytes_transferred: 10,
            total_bytes: 0,
            completed: false,
            error: None,
            local_path: "local".into(),
            remote_path: "remote".into(),
        };
        assert_eq!(zero.progress_fraction(), 0.0);
        assert_eq!(zero.direction_label(), "下载");

        let over = TransferItem {
            id: "o".into(),
            session_id: "s".into(),
            file_name: "o".into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 200,
            total_bytes: 100,
            completed: false,
            error: None,
            local_path: "local".into(),
            remote_path: "remote".into(),
        };
        assert_eq!(over.progress_fraction(), 1.0);
    }

    #[test]
    fn perm_label_handles_all_bits_off() {
        assert_eq!(entry(false, 0, 0).perm_label(), "----------");
        assert_eq!(entry(true, 0, 0).perm_label(), "d---------");
    }

    #[test]
    fn sort_directories_first_then_by_key() {
        let mk = |name: &str, dir: bool, size: u64| SftpFileEntry {
            name: name.into(),
            path: format!("/{name}"),
            is_dir: dir,
            size_bytes: size,
            modified_unix: 0,
            permissions: if dir { 0o755 } else { 0o644 },
            owner: None,
            group: None,
        };
        let mut list = vec![
            mk("b-file", false, 10),
            mk("a-dir", true, 0),
            mk("c-big", false, 999),
        ];
        sort_entries(&mut list, "size");
        assert!(list[0].is_dir, "目录应排最前");
        assert_eq!(list[0].name, "a-dir");
        assert_eq!(list[1].name, "c-big", "大小应降序");
        assert_eq!(list[2].name, "b-file");
        sort_entries(&mut list, "name");
        assert_eq!(list[0].name, "a-dir");
        assert_eq!(list[1].name, "b-file", "名称应字典序");
        assert_eq!(list[2].name, "c-big");
    }
}
