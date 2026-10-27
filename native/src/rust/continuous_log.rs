//! One bounded output queue per recording; only the worker owns disk handles.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::Duration;

const CHUNK: usize = 8192;
const QUEUE: usize = 32;
#[cfg(test)]
const ROTATE_BYTES: u64 = 8 * 1024 * 1024;
#[cfg(test)]
const KEEP_FILES: usize = 5;
static NEXT_RECORDING: AtomicU64 = AtomicU64::new(0);
static RECORDINGS: Mutex<Vec<Weak<Mutex<Status>>>> = Mutex::new(Vec::new());
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct WorkerPermit;
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Default)]
pub(super) struct Status {
    /// 0 idle, 1 starting, 2 writing, 3 paused (supplied by owner), 4 draining,
    /// 5 stopped, 6 failed. Error text is static; paths stay in the user UI.
    pub state: u8,
    pub path: String,
    pub error: String,
    pub drained: bool,
    pub written: u64,
    pub retained: u64,
}

pub(super) struct Recording {
    sender: Option<mpsc::SyncSender<Vec<u8>>>,
    status: Arc<Mutex<Status>>,
}

struct RotatingFile {
    root: PathBuf,
    file: Option<std::fs::File>,
    bytes: u64,
    limit: u64,
    keep: usize,
}

impl RotatingFile {
    fn open(root: &Path, limit: u64, keep: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("output.log"))?;
        Ok(Self {
            root: root.into(),
            file: Some(file),
            bytes: 0,
            limit,
            keep,
        })
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.sync_data()?;
        }
        let oldest = self.root.join(format!("output.{}.log", self.keep - 1));
        match std::fs::remove_file(oldest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        for index in (1..self.keep - 1).rev() {
            let source = self.root.join(format!("output.{index}.log"));
            if source.exists() {
                std::fs::rename(source, self.root.join(format!("output.{}.log", index + 1)))?;
            }
        }
        std::fs::rename(self.root.join("output.log"), self.root.join("output.1.log"))?;
        self.file = Some(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.root.join("output.log"))?,
        );
        self.bytes = 0;
        Ok(())
    }

    fn write(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        while !bytes.is_empty() {
            if self.bytes == self.limit {
                self.rotate()?;
            }
            let size = bytes.len().min((self.limit - self.bytes) as usize);
            self.file.as_mut().unwrap().write_all(&bytes[..size])?;
            self.bytes += size as u64;
            bytes = &bytes[size..];
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = &mut self.file {
            file.flush()?;
        }
        Ok(())
    }
}

impl Recording {
    #[cfg(test)]
    pub fn start(root: &Path) -> Result<Self, String> {
        Self::configured(root, "session", 0, ROTATE_BYTES, KEEP_FILES)
    }

    pub fn configured(
        root: &Path,
        prefix: &str,
        format: u8,
        limit: u64,
        keep: usize,
    ) -> Result<Self, String> {
        let sequence = NEXT_RECORDING.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "{prefix}-{}-{}-{sequence}",
            chrono::Utc::now().format("%Y%m%d-%H%M%S%.6f"),
            std::process::id()
        );
        Self::at_format(root.join(name), limit, keep, format)
    }

    #[cfg(test)]
    fn at(root: PathBuf, limit: u64, keep: usize) -> Result<Self, String> {
        Self::at_format(root, limit, keep, 0)
    }

    fn at_format(root: PathBuf, limit: u64, keep: usize, format: u8) -> Result<Self, String> {
        ACTIVE
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < 32).then_some(n + 1)
            })
            .map_err(|_| "session log worker limit reached")?;
        let permit = WorkerPermit;
        let status = Arc::new(Mutex::new(Status {
            state: 1,
            path: root.display().to_string(),
            error: String::new(),
            drained: false,
            written: 0,
            retained: 0,
        }));
        {
            let mut entries = RECORDINGS.lock().unwrap();
            entries.retain(|entry| entry.strong_count() > 0);
            entries.push(Arc::downgrade(&status));
        }
        let worker_status = status.clone();
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(QUEUE);
        std::thread::Builder::new()
            .name("session-log".into())
            .spawn(move || {
                let _permit = permit;
                let result = (|| -> std::io::Result<()> {
                    let mut writer = RotatingFile::open(&root, limit, keep)?;
                    {
                        let mut state = worker_status.lock().unwrap();
                        if state.state == 1 {
                            state.state = 2;
                        }
                    }
                    loop {
                        match receiver.recv_timeout(Duration::from_millis(250)) {
                            Ok(bytes) => {
                                let mut written = bytes.len() as u64;
                                if format == 1 {
                                    let stamp =
                                        format!("\n[{}] ", chrono::Local::now().to_rfc3339());
                                    writer.write(stamp.as_bytes())?;
                                    written += stamp.len() as u64;
                                }
                                writer.write(&bytes)?;
                                let mut state = worker_status.lock().unwrap();
                                state.written = state.written.saturating_add(written);
                                let rotated = state.written.saturating_sub(1) / limit;
                                state.retained =
                                    rotated.min((keep - 1) as u64) * limit + writer.bytes;
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => writer.flush()?,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    writer.flush()?;
                    if let Some(file) = &writer.file {
                        file.sync_data()?;
                    }
                    Ok(())
                })();
                let mut state = worker_status.lock().unwrap();
                if let Err(error) = result {
                    state.state = 6;
                    state.error = format!("session log write failed ({:?})", error.kind());
                } else if state.state != 6 {
                    state.state = 5;
                }
                state.drained = true;
            })
            .map_err(|_| "could not start session log worker")?;
        Ok(Self {
            sender: Some(sender),
            status,
        })
    }

    pub fn append(&mut self, data: &[u8]) {
        if self.status().state == 6 {
            self.sender.take();
            return;
        }
        for chunk in data.chunks(CHUNK) {
            let Some(sender) = &self.sender else {
                break;
            };
            if let Err(error) = sender.try_send(chunk.to_vec()) {
                let mut state = self.status.lock().unwrap();
                if state.state != 6 {
                    state.state = 6;
                    state.error = match error {
                        mpsc::TrySendError::Full(_) => "session log queue full; recording stopped",
                        _ => "session log worker stopped",
                    }
                    .into();
                }
                drop(state);
                self.sender.take();
                break;
            }
        }
    }

    pub fn stop(&mut self) {
        if self.sender.take().is_some() {
            let mut state = self.status.lock().unwrap();
            if state.state < 5 {
                state.state = 4;
            }
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn pending_drains() -> usize {
    RECORDINGS
        .lock()
        .unwrap()
        .iter()
        .filter_map(Weak::upgrade)
        .filter(|entry| {
            let state = entry.lock().unwrap();
            state.state >= 4 && !state.drained
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stopped(recording: &Recording) -> Status {
        let end = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let state = recording.status();
            if state.state >= 5 {
                return state;
            }
            assert!(std::time::Instant::now() < end, "log writer did not drain");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn continuous_file_rotates_in_order_and_drains_unterminated_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recording");
        let mut recording = Recording::at(root.clone(), 8, 3).unwrap();
        recording.append(b"0123456789abcdefghijklmnop");
        recording.stop();
        assert_eq!(stopped(&recording).state, 5);
        assert_eq!(
            std::fs::read(root.join("output.2.log")).unwrap(),
            b"89abcdef"
        );
        assert_eq!(
            std::fs::read(root.join("output.1.log")).unwrap(),
            b"ghijklmn"
        );
        assert_eq!(std::fs::read(root.join("output.log")).unwrap(), b"op");
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 3);
    }

    #[test]
    fn disk_failure_is_reported_without_replacing_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let occupied = dir.path().join("occupied");
        std::fs::write(&occupied, b"keep").unwrap();
        let mut recording = Recording::at(occupied.clone(), 8, 3).unwrap();
        recording.append(b"new data");
        recording.stop();
        assert_eq!(stopped(&recording).state, 6);
        assert!(!recording.status().error.is_empty());
        assert_eq!(std::fs::read(occupied).unwrap(), b"keep");
    }
}
