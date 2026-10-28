//! Local shell transport — PTY via `portable-pty`.
//!
//! Implements the [`Protocol`] trait for local shell processes
//! (PowerShell, bash, zsh, cmd, etc.). Migrated from the Tauri app crate.

use crate::error::{Error, Result};
use crate::protocol::Protocol;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

mod shell;

/// A local PTY shell session.
pub struct PtyInstance {
    /// Master side of the PTY — used for resize.
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// Write half (owned, behind a mutex). Wrapped in `Arc` so it can
    /// be moved into a `spawn_blocking` task for blocking writes.
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    /// Read half (owned, behind an async mutex for cooperative access).
    /// Wrapped in `Arc` so it can be moved into a `spawn_blocking` task
    /// for blocking reads.
    reader: Arc<AsyncMutex<Box<dyn std::io::Read + Send>>>,
    /// Whether the child process is still alive.
    alive: Arc<AtomicBool>,
    /// Child process exit code (set when the process exits).
    exit_code: Arc<Mutex<Option<u32>>>,
    /// Child process handle. Kept so `Drop` can `kill()` the shell instead
    /// of leaving it as an orphan when the session is destroyed. The reaper
    /// thread polls `try_wait`; it is set to `None` once the child exits.
    child: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>>,
    #[cfg(target_os = "linux")]
    working_directory: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl PtyInstance {
    /// Spawn a local shell process with the given terminal dimensions.
    ///
    /// Uses the system default shell when `shell` is `"auto"`.
    pub fn spawn(cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::spawn_with_options(cols, rows, shell, &[], None, "xterm-256color")
    }

    /// Spawn a local shell with explicit arguments and working directory.
    /// `term` 决定子进程的 `TERM` 环境变量（会话表单「终端:(T)」）。
    /// Explicit arguments replace named-shell defaults (e.g. bash login flags).
    pub fn spawn_with_options(
        cols: u16,
        rows: u16,
        shell: &str,
        args: &[String],
        cwd: Option<&str>,
        term: &str,
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::Pty(format!("openpty: {e}")))?;

        // Determine which shell to spawn
        let (program, base_args) = shell::resolve(shell)?;

        let mut cmd = CommandBuilder::new(&program);
        if args.is_empty() {
            cmd.args(&base_args);
        } else {
            cmd.args(args);
        }
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }

        // Set TERM environment variable
        cmd.env(
            "TERM",
            if term.is_empty() {
                "xterm-256color"
            } else {
                term
            },
        );

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| Error::Pty(format!("spawn {program}: {e}")))?;
        #[cfg(target_os = "linux")]
        let shell_pid = child.process_id();

        // Drop the slave to allow the master to detect EOF
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| Error::Pty(format!("clone reader: {e}")))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| Error::Pty(format!("take writer: {e}")))?;

        let master = Arc::new(Mutex::new(pair.master));
        let alive = Arc::new(AtomicBool::new(true));
        let exit_code = Arc::new(Mutex::new(None));
        let child = Arc::new(Mutex::new(Some(child)));
        #[cfg(target_os = "linux")]
        let working_directory = Arc::new(Mutex::new(None));
        #[cfg(target_os = "linux")]
        let directory_snapshot = working_directory.clone();

        // Spawn a background thread to reap the child. It polls `try_wait`
        // (never holding the child mutex during a blocking wait) so that
        // `Drop` can always acquire the lock and `kill()` a still-running
        // child. `child` is set to `None` once the process exits.
        let child_clone = child.clone();
        let alive_clone = alive.clone();
        let exit_code_clone = exit_code.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "linux")]
            let mut directory_poll = std::time::Instant::now();
            loop {
                if !alive_clone.load(Ordering::SeqCst) {
                    break;
                }
                let exited = {
                    let mut guard = match child_clone.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    match guard.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                        Some(status) => {
                            let code = status.exit_code();
                            if let Ok(mut ec) = exit_code_clone.lock() {
                                *ec = Some(code);
                            }
                            true
                        }
                        None => false,
                    }
                };
                if exited {
                    // Consume the reaped child so `Drop` does not double-wait.
                    if let Ok(mut guard) = child_clone.lock() {
                        *guard = None;
                    }
                    alive_clone.store(false, Ordering::SeqCst);
                    break;
                }
                // Reuse the lifetime thread; the UI only reads a cached path. This
                // follows an unmodified bash/zsh/fish, including explicit argv.
                #[cfg(target_os = "linux")]
                if directory_poll.elapsed() >= Duration::from_millis(200) {
                    directory_poll = std::time::Instant::now();
                    let path = shell_pid
                        .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok());
                    if let Ok(mut snapshot) = directory_snapshot.lock() {
                        *snapshot = path;
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        Ok(Self {
            master,
            writer: Arc::new(Mutex::new(writer)),
            reader: Arc::new(AsyncMutex::new(reader)),
            alive,
            exit_code,
            child,
            #[cfg(target_os = "linux")]
            working_directory,
        })
    }
}

#[async_trait::async_trait]
impl Protocol for PtyInstance {
    async fn read(&self) -> Result<Vec<u8>> {
        // Critical 3: PTY read is blocking IO; offload it to a blocking
        // thread pool so the async runtime is not stalled.
        let reader = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 8192];
            // Hold the lock only for the duration of the read.
            // The async mutex is fine here because we are inside a
            // blocking thread and do not need to await anything.
            let mut guard = reader.blocking_lock();
            let n = guard
                .read(&mut buf)
                .map_err(|e| Error::Pty(format!("read: {e}")))?;
            buf.truncate(n);
            Ok(buf)
        })
        .await
        .map_err(|e| Error::Pty(format!("read task join: {e}")))?
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        // R8: PTY write is blocking IO; offload it to a blocking thread pool.
        let writer = self.writer.clone();
        let alive = self.alive.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut guard = writer
                .lock()
                .map_err(|e| Error::Pty(format!("writer lock poisoned: {e}")))?;
            if !alive.load(Ordering::SeqCst) {
                return Err(Error::Pty("write: session is closed".into()));
            }
            guard
                .write_all(&data)
                .map_err(|e| Error::Pty(format!("write: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Pty(format!("write task join: {e}")))?
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        // TD-01: PTY resize holds a synchronous Mutex lock; offload it to
        // a blocking thread pool so the async runtime is not stalled.
        let master = self.master.clone();
        tokio::task::spawn_blocking(move || {
            let guard = master
                .lock()
                .map_err(|e| Error::Pty(format!("master lock poisoned: {e}")))?;
            guard
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| Error::Pty(format!("resize: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Pty(format!("resize task join: {e}")))?
    }

    async fn disconnect(&self) -> Result<()> {
        self.alive.store(false, Ordering::SeqCst);
        let child = self.child.clone();
        let exit_code = self.exit_code.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(mut process) = child
                .lock()
                .map_err(|e| Error::Pty(format!("child lock poisoned: {e}")))?
                .take()
            {
                let _ = process.kill(); // already exited is a normal disconnect race
                let status = process
                    .wait()
                    .map_err(|e| Error::Pty(format!("wait shell: {e}")))?;
                *exit_code
                    .lock()
                    .map_err(|e| Error::Pty(format!("exit code lock poisoned: {e}")))? =
                    Some(status.exit_code());
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::Pty(format!("disconnect task join: {e}")))?
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn exit_code(&self) -> Option<u32> {
        self.exit_code.lock().ok().and_then(|ec| *ec)
    }

    #[cfg(target_os = "linux")]
    fn local_working_directory(&self) -> Option<std::path::PathBuf> {
        if !self.is_alive() {
            return None;
        }
        self.working_directory.lock().ok().and_then(|p| p.clone())
    }
}

/// Ensure the PTY master and child process are cleaned up when the
/// session is dropped.
///
/// `Drop` also `kill()`s the child shell so a destroyed session does not
/// leave an orphan process behind. The reaper thread may still be polling
/// `try_wait`; taking the child out of the mutex and reaping it here makes
/// the reaper exit on its next iteration (child is `None`).
impl Drop for PtyInstance {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock()
            && let Some(mut c) = child.take()
        {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests;
