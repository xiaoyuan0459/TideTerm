//! Explicit snapshot IO, bounded pagination and external pager ownership per pane.
use super::{history_file::HistoryFile, term::try_with_tab};
use crate::bridge::reading::reading_ffi::{ExportState, HistoryText};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use tiderm_terminal::history_snapshot::HistorySnapshot;

enum Event {
    Ready(HistoryFile),
    Saved,
    PagerDone,
    Failed(String),
    Cancelled,
}
#[derive(Default)]
struct Control {
    cancelled: AtomicBool,
    commit: Mutex<()>,
}
impl Control {
    fn cancel(&self) {
        let _commit = self.commit.lock().unwrap();
        self.cancelled.store(true, Ordering::Release);
    }
}
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct Export {
    generation: u32,
    cancelled: Arc<Control>,
    progress: Arc<AtomicUsize>,
    rows: usize,
    receiver: mpsc::Receiver<Event>,
    sender: mpsc::Sender<Event>,
    file: Option<HistoryFile>,
    state: u8,
    error: String,
    permit: Arc<Permit>,
}
impl Drop for Export {
    fn drop(&mut self) {
        self.cancelled.cancel();
    }
}
impl Export {
    fn start(
        snapshot: HistorySnapshot,
        generation: u32,
        target: String,
        limit: u64,
        map: bool,
    ) -> Result<Self, String> {
        ACTIVE
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| "at most four history snapshots may be open")?;
        let permit = Arc::new(Permit);
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(Control::default());
        let progress = Arc::new(AtomicUsize::new(0));
        let rows = snapshot.rows();
        let (tx, cancel, completed) = (sender.clone(), cancelled.clone(), progress.clone());
        let worker_permit = permit.clone();
        std::thread::Builder::new()
            .name("history-export".into())
            .spawn(move || {
                let result = write(snapshot, &target, limit, map, &cancel, &completed);
                let event = if cancel.cancelled.load(Ordering::Acquire)
                    && !matches!(&result, Ok(Event::Saved))
                {
                    Event::Cancelled
                } else {
                    result.unwrap_or_else(Event::Failed)
                };
                let _ = tx.send(event);
                drop(worker_permit);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            generation,
            cancelled,
            progress,
            rows,
            receiver,
            sender,
            file: None,
            state: 1,
            error: String::new(),
            permit,
        })
    }
    fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Ready(file) => {
                    self.file = Some(file);
                    self.state = 2;
                }
                Event::Saved | Event::PagerDone => self.state = 4,
                Event::Failed(error) => {
                    self.error = error;
                    self.state = 5;
                }
                Event::Cancelled => self.state = 6,
            }
        }
    }
}

fn write(
    snapshot: HistorySnapshot,
    target: &str,
    limit: u64,
    map: bool,
    cancel: &Control,
    progress: &AtomicUsize,
) -> Result<Event, String> {
    let mut file = if target.is_empty() {
        tempfile::Builder::new()
            .prefix("tideterm-history-")
            .suffix(".txt")
            .tempfile()
    } else {
        let path = Path::new(target);
        if !path.is_absolute() {
            return Err("export path must be absolute".into());
        }
        tempfile::NamedTempFile::new_in(path.parent().ok_or("missing export directory")?)
    }
    .map_err(|e| e.to_string())?;
    let bytes = {
        let mut output = BufWriter::with_capacity(64 << 10, file.as_file_mut());
        let bytes = snapshot
            .write_utf8(&mut output, limit, |row| {
                progress.store(row, Ordering::Relaxed);
                !cancel.cancelled.load(Ordering::Acquire)
            })
            .map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        bytes
    };
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    // Cancellation and the atomic file replacement have a single commit boundary.
    let _commit = cancel.commit.lock().unwrap();
    if cancel.cancelled.load(Ordering::Acquire) {
        return Ok(Event::Cancelled);
    }
    if target.is_empty() {
        Ok(Event::Ready(HistoryFile::new(file, bytes, map)?))
    } else {
        file.persist(target).map_err(|e| e.error.to_string())?;
        Ok(Event::Saved)
    }
}

pub fn history_export_start(source: u32, target: &str, limit_mb: u32, map: bool) -> String {
    if super::term_ops::input_locked() {
        return "input locked".into();
    }
    if !(4..=2048).contains(&limit_mb) {
        return "invalid disk limit".into();
    }
    try_with_tab(source, |st| {
        if st
            .history_export
            .as_ref()
            .is_some_and(|j| matches!(j.state, 1 | 3))
        {
            return "history task already running".into();
        }
        let Some(snapshot) = st.pane.history_snapshot() else {
            return "leave alternate screen before exporting".into();
        };
        match Export::start(
            snapshot,
            st.generation,
            target.into(),
            u64::from(limit_mb) << 20,
            map,
        ) {
            Ok(job) => {
                st.history_export = Some(job);
                String::new()
            }
            Err(error) => error,
        }
    })
    .unwrap_or_else(|| "source closed".into())
}

pub fn history_export_poll(source: u32) -> ExportState {
    if super::term_ops::input_locked() {
        history_export_close(source);
        return ExportState::default();
    }
    try_with_tab(source, |st| {
        if st
            .history_export
            .as_ref()
            .is_some_and(|j| j.generation != st.generation)
        {
            st.history_export = None;
        }
        let Some(job) = &mut st.history_export else {
            return ExportState::default();
        };
        job.poll();
        ExportState {
            state: job.state,
            completed: job.progress.load(Ordering::Relaxed) as u32,
            rows: job.rows as u32,
            bytes: job.file.as_ref().map_or(0, |f| f.bytes),
            mapped: job.file.as_ref().is_some_and(HistoryFile::mapped),
            error: job.error.clone(),
        }
    })
    .unwrap_or_default()
}

pub fn history_export_page(source: u32, page: u32) -> HistoryText {
    if history_export_poll(source).state != 2 {
        return HistoryText::default();
    }
    try_with_tab(source, |st| {
        let file = st.history_export.as_mut()?.file.as_mut()?;
        match file.page(page) {
            Ok(text) => Some(HistoryText {
                available: true,
                first_line: u64::from(page) + 1,
                text,
                truncated: false,
            }),
            Err(error) => {
                st.history_export.as_mut()?.error = error;
                st.history_export.as_mut()?.state = 5;
                None
            }
        }
    })
    .flatten()
    .unwrap_or_default()
}

pub fn history_export_close(source: u32) {
    let _ = try_with_tab(source, |st| {
        st.history_export = None;
    });
}

pub fn history_export_pager(source: u32, executable: &str, kind: u8) -> String {
    if history_export_poll(source).state != 2 {
        return "snapshot unavailable".into();
    }
    if !Path::new(executable).is_absolute()
        || !Path::new(executable).is_file()
        || !matches!(kind, 1 | 2)
    {
        return "select an absolute less or nvim executable".into();
    }
    try_with_tab(source, |st| {
        let Some(job) = &mut st.history_export else {
            return "snapshot unavailable".into();
        };
        let Some(file) = job.file.take() else {
            return "snapshot unavailable".into();
        };
        let bytes = file.bytes;
        let file = file.into_file();
        let executable_path = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                job.file = HistoryFile::new(file, bytes, false).ok();
                return error.to_string();
            }
        };
        let mut command = std::process::Command::new(executable_path);
        command
            .arg("--history-pager-helper")
            .arg(kind.to_string())
            .arg(file.path())
            .arg(executable);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // The file owner itself has no window.
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                job.file = HistoryFile::new(file, bytes, false).ok();
                return error.to_string();
            }
        };
        let path = match file.into_temp_path().keep() {
            Ok(path) => path,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return error.to_string();
            }
        };
        let sender = job.sender.clone();
        let permit = job.permit.clone();
        // The child retains its private file until exit. Closing the source retires the
        // reply but never kills an editor with possible user work in it.
        super::session::shared_runtime().spawn_blocking(move || {
            let result = child.wait();
            // The helper also owns cleanup, so a normal GUI process exit cannot
            // strand the file. This owner additionally covers helper startup failure.
            drop(tempfile::TempPath::try_from_path(path));
            let _ = sender.send(match result {
                Ok(status) if status.success() => Event::PagerDone,
                Ok(status) => Event::Failed(format!("pager exited: {status}")),
                Err(error) => Event::Failed(error.to_string()),
            });
            drop(permit);
        });
        job.state = 3;
        String::new()
    })
    .unwrap_or_else(|| "source closed".into())
}

#[cfg(test)]
mod tests;
