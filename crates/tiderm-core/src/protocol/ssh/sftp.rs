//! SFTP transport implementation over libssh2.
//!
//! v0.10: replaces russh-sftp with `ssh2::Sftp`. One-shot operations run via
//! `spawn_blocking`; streaming file handles are wrapped in `AsyncRead`/
//! `AsyncWrite` adapters that perform blocking libssh2 reads/writes.

use crate::sftp::{SftpFileEntry, SftpFileStat, SftpTransport};
use std::io;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};

use super::connection::BlockingGuard;
mod listing;
mod metadata;
mod streams;
mod workflow;
use streams::{LibSftpReadStream, LibSftpWriteStream, SftpStreamFile};

/// Chunk size for the streaming read adapter.
const IO_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A blocking-mode switch must cover one complete libssh2 call. Every SFTP
/// operation on the SSH session uses the same gate, including file close.
/// Fields drop in declaration order: restore the mode before unlocking.
struct SftpCallGuard<'a> {
    _blocking: BlockingGuard<'a>,
    _gate: MutexGuard<'a, ()>,
}

impl<'a> SftpCallGuard<'a> {
    fn new(session: &'a ssh2::Session, gate: &'a Mutex<()>) -> Result<Self, String> {
        let lock = gate
            .lock()
            .map_err(|_| "SSH session blocking-mode lock poisoned".to_string())?;
        Ok(Self {
            _blocking: BlockingGuard::new(session),
            _gate: lock,
        })
    }
}

fn retry_io<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let deadline = Instant::now() + IO_STALL_TIMEOUT;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("SFTP made no progress for 30 seconds: {error}"),
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

/// SFTP transport backed by a `ssh2::Sftp`.
pub struct LibSshSftp {
    /// 所属 SSH 会话句柄。R-21 后交互会话跑在非阻塞模式，
    /// 一次性 SFTP 调用需要它临时切回阻塞（[`BlockingGuard`]）。
    session: ssh2::Session,
    blocking_mode_gate: Arc<Mutex<()>>,
    sftp: Arc<ssh2::Sftp>,
    alive: Arc<AtomicBool>,
    file_mode: i32,
    dir_mode: i32,
}

impl LibSshSftp {
    pub fn new(
        session: ssh2::Session,
        sftp: ssh2::Sftp,
        alive: Arc<AtomicBool>,
        blocking_mode_gate: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            session,
            blocking_mode_gate,
            sftp: Arc::new(sftp),
            alive,
            file_mode: 0o644,
            dir_mode: 0o755,
        }
    }

    pub fn with_creation_modes(mut self, file: u32, directory: u32) -> Self {
        self.file_mode = (file & 0o777) as i32;
        self.dir_mode = (directory & 0o777) as i32;
        self
    }
}

/// Detect directory from the file mode type bits (S_IFMT = 0o170000,
/// S_IFDIR = 0o040000).
fn is_dir(perm: Option<u32>) -> bool {
    perm.map(|p| (p & 0o170000) == 0o040000).unwrap_or(false)
}

#[async_trait::async_trait]
impl SftpTransport for LibSshSftp {
    async fn rename_new(&self, from: &str, to: &str) -> Result<(), String> {
        self.workflow_rename_new(from, to).await
    }
    async fn lstat_optional(&self, path: &str) -> Result<Option<SftpFileStat>, String> {
        self.workflow_lstat_optional(path).await
    }
    async fn open_write_new(
        &self,
        path: &str,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        self.workflow_write_new(path, self.file_mode).await
    }
    async fn open_write_new_with_mode(
        &self,
        path: &str,
        mode: u32,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        self.workflow_write_new(path, (mode & 0o7777) as i32).await
    }
    async fn set_permissions(&self, path: &str, mode: u32) -> Result<(), String> {
        self.update_permissions(path, mode).await
    }
    async fn symlink(&self, target: &str, link: &str) -> Result<(), String> {
        self.workflow_symlink(target, link).await
    }
    async fn list_dir_bounded(
        &self,
        path: &str,
        query: crate::sftp::DirectoryQuery,
    ) -> Result<crate::sftp::DirectorySnapshot, crate::sftp::DirectoryReadError> {
        self.list_bounded(path, query).await
    }
    async fn list_dir(&self, path: &str) -> std::result::Result<Vec<SftpFileEntry>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let entries = sftp.readdir(Path::new(&path)).map_err(|e| e.to_string())?;
            Ok(entries
                .into_iter()
                .map(|(p, st)| SftpFileEntry {
                    name: p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string(),
                    path: p.to_string_lossy().into_owned(),
                    is_dir: is_dir(st.perm),
                    size_bytes: st.size.unwrap_or(0),
                    modified_unix: st.mtime.unwrap_or(0) as i64,
                    permissions: st.perm.unwrap_or(0),
                    // libssh2 stat 只给数字 uid/gid（ssh2 crate 不暴露 longname，
                    // 用户名映射留待 UI 层或后续 vendor 补丁）。
                    owner: st.uid.map(|v| v.to_string()),
                    group: st.gid.map(|v| v.to_string()),
                })
                .collect())
        })
        .await
        .map_err(|e| format!("list_dir task panicked: {e}"))?
    }

    async fn mkdir(&self, path: &str) -> std::result::Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let mode = self.dir_mode;
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.mkdir(Path::new(&path), mode)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("mkdir task panicked: {e}"))?
    }

    async fn remove_file(&self, path: &str) -> std::result::Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.unlink(Path::new(&path)).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("remove_file task panicked: {e}"))?
    }

    async fn remove_dir(&self, path: &str) -> std::result::Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.rmdir(Path::new(&path)).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("remove_dir task panicked: {e}"))?
    }

    async fn rename(&self, from: &str, to: &str) -> std::result::Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let from = from.to_string();
        let to = to.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.rename(Path::new(&from), Path::new(&to), None)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("rename task panicked: {e}"))?
    }

    async fn read_file(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let mut file = sftp.open(Path::new(&path)).map_err(|e| e.to_string())?;
            let mut data = Vec::new();
            file.read_to_end(&mut data).map_err(|e| e.to_string())?;
            Ok(data)
        })
        .await
        .map_err(|e| format!("read_file task panicked: {e}"))?
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> std::result::Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let data = data.to_vec();
        let mode = self.file_mode;
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let mut file = sftp
                .open_mode(
                    Path::new(&path),
                    ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::TRUNCATE,
                    mode,
                    ssh2::OpenType::File,
                )
                .map_err(|e| e.to_string())?;
            file.write_all(&data).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("write_file task panicked: {e}"))?
    }

    async fn stat(&self, path: &str) -> std::result::Result<SftpFileStat, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let st = sftp.stat(Path::new(&path)).map_err(|e| e.to_string())?;
            Ok(SftpFileStat {
                accessed_unix: st.atime.map(|v| v as i64),
                size_bytes: st.size.unwrap_or(0),
                modified_unix: st.mtime.unwrap_or(0) as i64,
                permissions: st.perm.unwrap_or(0),
                is_dir: is_dir(st.perm),
                owner: None,
                group: None,
            })
        })
        .await
        .map_err(|e| format!("stat task panicked: {e}"))?
    }

    async fn set_times(
        &self,
        path: &str,
        accessed: Option<u64>,
        modified: Option<u64>,
    ) -> Result<(), String> {
        self.update_times(path, accessed, modified).await
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn open_read_stream(
        &self,
        path: &str,
    ) -> std::result::Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let file = tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.open(Path::new(&path)).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("open_read task panicked: {e}"))??;
        Ok(Box::new(LibSftpReadStream {
            inner: SftpStreamFile::new(file, self.session.clone(), self.blocking_mode_gate.clone()),
        }))
    }

    async fn open_write_stream(
        &self,
        path: &str,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let mode = self.file_mode;
        let file = tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.open_mode(
                Path::new(&path),
                ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::TRUNCATE,
                mode,
                ssh2::OpenType::File,
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("open_write task panicked: {e}"))??;
        Ok(Box::new(LibSftpWriteStream {
            inner: SftpStreamFile::new(file, self.session.clone(), self.blocking_mode_gate.clone()),
        }))
    }

    async fn open_read_stream_from(
        &self,
        path: &str,
        offset: u64,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let file = tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let mut file = sftp.open(Path::new(&path)).map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(file)
        })
        .await
        .map_err(|e| format!("open_read_from task panicked: {e}"))??;
        Ok(Box::new(LibSftpReadStream {
            inner: SftpStreamFile::new(file, self.session.clone(), self.blocking_mode_gate.clone()),
        }))
    }

    async fn open_write_stream_at(
        &self,
        path: &str,
        offset: u64,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        if offset == 0 {
            return self.open_write_stream(path).await;
        }
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_string();
        let file = tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let mut file = sftp
                .open_mode(
                    Path::new(&path),
                    ssh2::OpenFlags::WRITE,
                    0o644,
                    ssh2::OpenType::File,
                )
                .map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(file)
        })
        .await
        .map_err(|e| format!("open_write_at task panicked: {e}"))??;
        Ok(Box::new(LibSftpWriteStream {
            inner: SftpStreamFile::new(file, self.session.clone(), self.blocking_mode_gate.clone()),
        }))
    }
}
