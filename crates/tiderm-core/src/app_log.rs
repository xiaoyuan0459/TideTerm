//! Application-level structured logging (core-side pure logic).
//!
//! 复刻自旧 Tauri 后端 `src-tauri/src/app_log.rs` 里与 Tauri 无关的部分：
//! 日志目录、按修改时间保留最近 N 份的轮转、JSON 行日志读取、审计事件。
//! （旧版的 `tracing-appender` 文件初始化留在应用壳层——native 侧已有
//! `init_observability()`；本模块只承载可单测的纯逻辑。）
//!
//! ## 轮转策略（与旧版一致）
//! - 单文件上限：10 MB
//! - 保留最近 5 份
//!
//! ## 审计目标
//! 安全相关事件以 `target = "audit"` 记录，便于单独过滤/导出。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Maximum number of rotated log files to retain.
pub const MAX_LOG_FILES: usize = 5;
/// Single log file size cap (10 MB) — enforced by the shell layer.
pub const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// A single log entry returned by [`read_entries_from_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Severity: `TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`.
    pub level: String,
    /// Module path (e.g. `session`, `audit`).
    pub target: String,
    /// Human-readable message.
    pub message: String,
}

/// Return the log directory, creating it if necessary.
pub fn log_dir() -> PathBuf {
    let base = crate::config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("logs");
    let _ = fs::create_dir_all(&base);
    base
}

/// Remove old log files exceeding [`MAX_LOG_FILES`].
///
/// Keeps the newest files by modification time.  Called once at
/// startup; errors are logged but never propagated.
pub fn rotate_logs(dir: &Path) {
    let mut entries: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("tideterm.log"))
            })
            .collect(),
        Err(_) => return,
    };

    if entries.len() <= MAX_LOG_FILES {
        return;
    }

    // Sort by modification time, newest first.
    entries.sort_by(|a, b| {
        let ta = a.metadata().and_then(|m| m.modified()).ok();
        let tb = b.metadata().and_then(|m| m.modified()).ok();
        tb.cmp(&ta)
    });

    for old in entries.iter().skip(MAX_LOG_FILES) {
        if let Err(e) = fs::remove_file(old.path()) {
            tracing::warn!(
                target: "app",
                "rotate_logs: failed to remove {}: {e}",
                old.path().display()
            );
        }
    }
}

/// Read the most recent log entries from today's log file.
///
/// Returns up to `limit` entries whose level is >= `min_level`.
/// Parsing failures are silently skipped (a log reader must never
/// crash the application).
pub fn read_log_entries(min_level: Option<&str>, limit: Option<u32>) -> Vec<LogEntry> {
    read_entries_from_dir(&log_dir(), min_level, limit)
}

/// Shared implementation of [`read_log_entries`] with an injectable log
/// directory（旧版 P2-R3：合并了只为测试存在的影子实现）。
pub fn read_entries_from_dir(
    dir: &Path,
    min_level: Option<&str>,
    limit: Option<u32>,
) -> Vec<LogEntry> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_file = dir.join(format!("tideterm.log.{today}"));

    let file = match fs::File::open(&log_file) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let level_order = |l: &str| match l {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 5,
    };

    let threshold = level_order(min_level.unwrap_or("INFO"));
    let cap = limit.unwrap_or(200) as usize;

    let reader = BufReader::new(file);
    let mut entries: Vec<LogEntry> = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        // 每行是 tracing-subscriber JSON 格式化器产出的对象；这里只抽取需要的
        // 字段，不做完整反序列化。
        let parsed: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let level = parsed
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("INFO");

        if level_order(level) < threshold {
            continue;
        }

        entries.push(LogEntry {
            timestamp: parsed
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            level: level.to_string(),
            target: parsed
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            message: parsed
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        });

        if entries.len() >= cap {
            break;
        }
    }

    entries
}

/// Log a security-relevant audit event.
///
/// Usage: `audit_log("master_password_set", &[("user", "local")])`
///
/// Events are written to the structured log under `target = "audit"`
/// so they can be filtered independently.
pub fn audit_log(action: &str, fields: &[(&str, &str)]) {
    let kv: String = fields
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    tracing::info!(target: "audit", action = action, "{kv}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_log(dir: &Path, name: &str, lines: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        let mut f = fs::File::create(dir.join(name)).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn rotate_keeps_only_the_newest_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();

        // 7 份日志：给它们递增的修改时间（旧文件更早）。
        for i in 0..7 {
            // Filename order is the reverse of modification-time order.
            let path = dir.join(format!("tideterm.log.{}", 7 - i));
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "file-{i}").unwrap();
            let stamp = std::time::SystemTime::now() - std::time::Duration::from_secs(600 - i * 60);
            fs::set_times(&path, fs::FileTimes::new().set_modified(stamp)).unwrap();
        }
        // 干扰文件不应被计入轮转。
        write_log(dir, "unrelated.txt", &["keep me"]);

        rotate_logs(dir);
        let left = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_str().unwrap().starts_with("tideterm.log"))
            .count();
        assert_eq!(left, MAX_LOG_FILES, "应只保留最近 5 份");
        for i in 0..7 {
            assert_eq!(dir.join(format!("tideterm.log.{}", 7 - i)).exists(), i >= 2);
        }
        assert!(dir.join("unrelated.txt").exists());
    }

    #[test]
    fn read_entries_filters_level_and_limit() {
        let dir = std::env::temp_dir().join(format!("tiderm_read_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        write_log(
            &dir,
            &format!("tideterm.log.{today}"),
            &[
                r#"{"timestamp":"t1","level":"DEBUG","target":"session","message":"d"}"#,
                r#"{"timestamp":"t2","level":"INFO","target":"session","message":"i1"}"#,
                r#"{"timestamp":"t3","level":"WARN","target":"audit","message":"w"}"#,
                r#"not json at all"#,
            ],
        );
        let all = read_entries_from_dir(&dir, Some("INFO"), None);
        assert_eq!(all.len(), 2, "DEBUG 应被过滤，坏行应被跳过");
        assert_eq!(all[0].message, "i1");
        assert_eq!(all[1].level, "WARN");
        assert_eq!(all[1].target, "audit");

        let one = read_entries_from_dir(&dir, Some("INFO"), Some(1));
        assert_eq!(one.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn audit_log_does_not_panic() {
        audit_log("master_password_throttle_engaged", &[]);
        audit_log("master_password_set", &[("user", "local")]);
    }
}
