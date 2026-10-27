//! 会话日志域（Phase C ⑧；对齐 Slint 壳 shell/session_log.rs 语义）：
//! 每标签 ring buffer（2000 行，`HH:MM:SS` 时间戳前缀）+ 捕获开关 + 导出。
//!
//! 数据流：`pump_all` 的 `feed_bytes` 处（会话输出唯一入口）按开关 append；
//! 导出 = 全环写 `<config>/logs/tiderm_<tag>_<stamp>.log`。⑨ LogView 的
//! 读面（recent/count）后续按需补桥，本文件先落存储与导出。

use std::collections::VecDeque;

/// 每会话保留行数上限（ring buffer；旧行先出——Slint 壳 LOG_LINES_CAP 同值）。
pub(crate) const LOG_LINES_CAP: usize = 2000;
const LOG_LINE_BYTES_CAP: usize = 4096;

/// 本地墙钟标签（`HH:MM:SS`；chrono 与 session_store 的 iso_now 同依赖）。
fn local_ts() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// 一个标签的会话日志（ring + 行切分 pending + 捕获开关）。
pub(crate) struct SessionLog {
    on: bool,
    ring: VecDeque<(String, String)>,
    /// 未见到行尾的尾巴（跨 chunk 行切分，append 时续接）。
    pending: String,
    revision: u64,
    recording: Option<super::continuous_log::Recording>,
    paused: bool,
    startup_error: String,
}

impl SessionLog {
    pub(crate) fn new() -> Self {
        Self {
            on: false,
            ring: VecDeque::new(),
            pending: String::new(),
            revision: 0,
            recording: None,
            paused: false,
            startup_error: String::new(),
        }
    }

    pub(crate) fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.paused = false;
            if let Some(recording) = &mut self.recording {
                recording.stop();
            }
        }
    }

    pub(crate) fn start_disk(&mut self) -> Result<(), String> {
        if self
            .recording
            .as_ref()
            .is_some_and(|r| r.status().state == 4)
        {
            return Err("previous session log is still draining".into());
        }
        if self
            .recording
            .as_ref()
            .is_some_and(|r| r.status().state < 4)
        {
            self.on = true;
            self.paused = false;
            return Ok(());
        }
        let result = super::log_settings::recording();
        self.recording = match result {
            Ok(recording) => {
                self.startup_error.clear();
                Some(recording)
            }
            Err(error) => {
                self.startup_error = error.clone();
                self.on = false;
                return Err(error);
            }
        };
        self.on = true;
        self.paused = false;
        Ok(())
    }

    pub(crate) fn pause(&mut self, paused: bool) {
        if self.on {
            self.paused = paused;
        }
    }

    pub(super) fn disk_status(&self) -> super::continuous_log::Status {
        if !self.startup_error.is_empty() {
            return super::continuous_log::Status {
                state: 6,
                error: self.startup_error.clone(),
                drained: true,
                ..Default::default()
            };
        }
        let mut state = self
            .recording
            .as_ref()
            .map(|r| r.status())
            .unwrap_or_default();
        if state.state == 2 && self.paused {
            state.state = 3;
        }
        state
    }

    pub(crate) fn on(&self) -> bool {
        self.on && self.disk_status().state != 6
    }

    /// 追加会话输出字节：UTF-8 lossy + 按 \n 切行（\r 去尾），整行进环。
    pub(crate) fn append(&mut self, data: &[u8]) {
        if !self.on || self.paused {
            return;
        }
        if let Some(recording) = &mut self.recording {
            recording.append(data);
        }
        let decoded = String::from_utf8_lossy(data);
        for fragment in decoded.split_inclusive('\n') {
            let complete = fragment.ends_with('\n');
            let fragment = fragment.trim_end_matches(['\n', '\r']);
            let available = LOG_LINE_BYTES_CAP.saturating_sub(self.pending.len());
            let mut used = available.min(fragment.len());
            while !fragment.is_char_boundary(used) {
                used -= 1;
            }
            self.pending.push_str(&fragment[..used]);
            if complete {
                if !self.pending.is_empty() {
                    self.ring
                        .push_back((local_ts(), std::mem::take(&mut self.pending)));
                    if self.ring.len() > LOG_LINES_CAP {
                        self.ring.pop_front();
                    }
                    self.revision = self.revision.wrapping_add(1);
                }
                self.pending.clear();
            }
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn recent(&self, limit: usize) -> String {
        self.ring
            .iter()
            .rev()
            .take(limit.min(200))
            .rev()
            .map(|(ts, line)| format!("[{ts}] {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn clear(&mut self) {
        self.ring.clear();
        self.pending.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    /// 全环快照（导出/后续 LogView 读面）。
    pub(crate) fn lines(&self) -> Vec<(String, String)> {
        self.ring.iter().cloned().collect()
    }

    /// 导出为 `<config>/logs/tiderm_<tag>_<stamp>.log`，返回路径。
    pub(crate) fn export(&self, tag: &str) -> Result<std::path::PathBuf, String> {
        if self.ring.is_empty() {
            return Err("当前会话没有日志".to_string());
        }
        let dir = std::path::PathBuf::from(super::log_settings::slog_settings()?.directory);
        self.export_into(&dir, tag)
    }

    fn export_into(&self, dir: &std::path::Path, tag: &str) -> Result<std::path::PathBuf, String> {
        if self.ring.is_empty() {
            return Err("当前会话没有日志".to_string());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S%.9f");
        let safe_tag: String = tag
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(48)
            .collect();
        let path = dir.join(format!("tiderm_{safe_tag}_{stamp}.log"));
        let mut body = String::new();
        for (ts, line) in self.lines() {
            body.push_str(&format!("[{ts}] {line}\n"));
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_only_when_on_and_splits_lines_across_chunks() {
        let mut log = SessionLog::new();
        log.append(b"hello\nwor");
        assert!(log.lines().is_empty(), "关态不捕获");

        log.set_on(true);
        log.append(b"hello\nwor");
        assert_eq!(log.lines().len(), 1, "整行进环");

        log.append(b"ld\n");
        assert_eq!(log.lines().len(), 2, "跨 chunk 行切分后成行");
        assert_eq!(log.lines()[1].1, "world", "pending 续接成完整行");
    }

    #[test]
    fn ring_cap_drops_oldest() {
        let mut log = SessionLog::new();
        log.set_on(true);
        for i in 0..(LOG_LINES_CAP + 50) {
            log.append(format!("line-{i}\n").as_bytes());
        }
        let lines = log.lines();
        assert_eq!(lines.len(), LOG_LINES_CAP);
        assert_eq!(lines[0].1, "line-50", "最老的 50 行被挤出");
    }

    #[test]
    fn long_unterminated_output_stays_bounded() {
        let mut log = SessionLog::new();
        log.set_on(true);
        log.append(&vec![b'x'; LOG_LINE_BYTES_CAP * 4]);
        assert_eq!(log.pending.len(), LOG_LINE_BYTES_CAP);
        log.append(b"\nnext\n");
        assert_eq!(log.lines()[0].1.len(), LOG_LINE_BYTES_CAP);
        assert!(log.recent(1).ends_with("next"));
        log.clear();
        assert!(log.lines().is_empty());
    }

    #[test]
    fn export_writes_timestamped_body() {
        let mut log = SessionLog::new();
        assert!(log.export("t").is_err(), "空环导出报错");

        log.set_on(true);
        log.append(b"captured line\n");
        let dir = std::env::temp_dir().join(format!("tideterm-log-test-{}", std::process::id()));
        let path = log.export_into(&dir, "probe").expect("export");
        let body = std::fs::read_to_string(&path).expect("read");
        assert!(body.contains("captured line"));
        assert!(body.starts_with('['), "每行带时间戳前缀");
        log.clear();
        log.append(b"second export\n");
        let second = log.export_into(&dir, "probe").expect("second export");
        assert_ne!(
            path, second,
            "exports must not overwrite an earlier snapshot"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        std::fs::remove_file(second).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn disk_recording_survives_preview_eviction_and_pause_does_not_capture_output() {
        let dir = tempfile::tempdir().unwrap();
        let recording = super::super::continuous_log::Recording::start(dir.path()).unwrap();
        let path = std::path::PathBuf::from(recording.status().path).join("output.log");
        let mut log = SessionLog::new();
        log.recording = Some(recording);
        log.set_on(true);
        let output: String = (0..2050)
            .map(|index| format!("fixture-{index}\n"))
            .collect();
        log.append(output.as_bytes());
        log.pause(true);
        log.append(b"paused-private-output\n");
        log.pause(false);
        log.append(b"unterminated-tail");
        log.set_on(false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !log.disk_status().drained {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(log.disk_status().state, 5, "{}", log.disk_status().error);
        let output = std::fs::read_to_string(path).unwrap();
        assert!(output.starts_with("fixture-0\n"));
        assert!(output.ends_with("unterminated-tail"));
        assert!(!output.contains("paused-private-output"));
        assert_eq!(log.lines().len(), LOG_LINES_CAP);
    }
}
