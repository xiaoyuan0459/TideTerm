use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tiderm_core::sftp::{DirectoryQuery, SftpTransport};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
static BACKUPS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(super) enum Endpoint {
    Local,
    Remote(Arc<dyn SftpTransport>, Arc<AtomicBool>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Metadata {
    pub kind: u8,
    pub size: u64,
    pub modified: i128,
}

impl Endpoint {
    pub fn for_uid(uid: &str) -> Result<Self, String> {
        if uid.is_empty() {
            Ok(Self::Local)
        } else {
            let (channel, live) = super::sftp::workflow_endpoint(uid)?;
            Ok(Self::Remote(channel, live))
        }
    }
    pub fn check(&self) -> Result<(), String> {
        if let Self::Remote(_, live) = self
            && !live.load(Ordering::SeqCst)
        {
            return Err("source session closed".into());
        }
        Ok(())
    }
    pub async fn metadata(&self, path: &str) -> Result<Option<Metadata>, String> {
        self.check()?;
        match self {
            Self::Local => match tokio::fs::symlink_metadata(path).await {
                Ok(meta) => Ok(Some(Metadata {
                    kind: if meta.file_type().is_symlink() {
                        2
                    } else if meta.is_dir() {
                        1
                    } else {
                        0
                    },
                    size: meta.len(),
                    modified: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_nanos() as i128),
                })),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            Self::Remote(channel, _) => Ok(channel.lstat_optional(path).await?.map(|m| Metadata {
                kind: if m.permissions & 0o170000 == 0o120000 {
                    2
                } else if m.is_dir {
                    1
                } else {
                    0
                },
                size: m.size_bytes,
                modified: i128::from(m.modified_unix),
            })),
        }
    }
    pub async fn list(&self, path: &str, cancel: Arc<AtomicBool>) -> Result<Vec<String>, String> {
        self.check()?;
        let mut names = Vec::new();
        match self {
            Self::Local => {
                let mut dir = tokio::fs::read_dir(path).await.map_err(|e| e.to_string())?;
                while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
                    if names.len() == 2048 {
                        return Err("directory exceeds 2048-entry budget".into());
                    }
                    names.push(
                        entry
                            .file_name()
                            .into_string()
                            .map_err(|_| "non-UTF8 file name")?,
                    );
                }
            }
            Self::Remote(channel, _) => {
                let snapshot = channel
                    .list_dir_bounded(
                        path,
                        DirectoryQuery {
                            max_entries: 2048,
                            max_name_bytes: 262144,
                            deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
                            canceled: cancel,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                if snapshot.truncated {
                    return Err("directory exceeds listing budget".into());
                }
                names = snapshot
                    .entries
                    .into_iter()
                    .map(|entry| entry.name)
                    .collect();
            }
        }
        for name in &names {
            safe_name(name)?;
        }
        self.check()?;
        Ok(names)
    }
    pub async fn mkdir(&self, path: &str) -> Result<(), String> {
        self.check()?;
        match self {
            Self::Local => tokio::fs::create_dir(path).await.map_err(|e| e.to_string()),
            Self::Remote(c, _) => c.mkdir(path).await,
        }
    }
    pub async fn remove(&self, path: &str, kind: u8) -> Result<(), String> {
        self.check()?;
        match (self, kind) {
            (Self::Local, 1) => tokio::fs::remove_dir(path).await.map_err(|e| e.to_string()),
            (Self::Local, _) => tokio::fs::remove_file(path)
                .await
                .map_err(|e| e.to_string()),
            (Self::Remote(c, _), 1) => c.remove_dir(path).await,
            (Self::Remote(c, _), _) => c.remove_file(path).await,
        }
    }
    pub async fn replace_file(&self, source: &str, target: &str) -> Result<(), String> {
        self.check()?;
        match self {
            Self::Local => tokio::fs::rename(source, target)
                .await
                .map_err(|e| e.to_string()),
            Self::Remote(c, _) => {
                // SFTP v3 RENAME cannot overwrite. Preserve the old file until
                // commit succeeds, and never overwrite a concurrent replacement.
                let backup = format!(
                    "{target}.tideterm-{}-{}-{}.bak",
                    std::process::id(),
                    chrono::Utc::now().timestamp_micros(),
                    BACKUPS.fetch_add(1, Ordering::Relaxed)
                );
                c.rename_new(target, &backup).await?;
                if let Err(error) = c.rename_new(source, target).await {
                    return match c.rename_new(&backup, target).await {
                        Ok(()) => Err(format!("replacement failed; original restored: {error}")),
                        Err(restore) => Err(format!("replacement failed: {error}; original retained at {backup}; restore failed: {restore}")),
                    };
                }
                c.remove_file(&backup).await.map_err(|error| {
                    format!("replacement saved; original backup remains at {backup}: {error}")
                })
            }
        }
    }
    pub async fn rename_new(&self, source: &str, target: &str) -> Result<(), String> {
        self.check()?;
        if let Self::Remote(c, _) = self {
            return c.rename_new(source, target).await;
        }
        let source = source.to_owned();
        let target = target.to_owned();
        tokio::task::spawn_blocking(move || {
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                let from: Vec<u16> = std::ffi::OsStr::new(&source)
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                let to: Vec<u16> = std::ffi::OsStr::new(&target)
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                // SAFETY: both paths are owned, terminated UTF-16 buffers valid for this call.
                let ok = unsafe {
                    windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                        from.as_ptr(),
                        to.as_ptr(),
                        0,
                    )
                };
                if ok != 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error().to_string())
                }
            }
            #[cfg(target_os = "linux")]
            {
                let from = std::ffi::CString::new(source).map_err(|_| "invalid source")?;
                let to = std::ffi::CString::new(target).map_err(|_| "invalid target")?;
                // SAFETY: terminated paths are valid, and no-replace is enforced by the filesystem.
                let result = unsafe {
                    libc::renameat2(
                        libc::AT_FDCWD,
                        from.as_ptr(),
                        libc::AT_FDCWD,
                        to.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error().to_string())
                }
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                let _ = (source, target);
                Err("non-replacing move is unsupported on this platform".into())
            }
        })
        .await
        .map_err(|_| "move task failed")?
    }
    pub async fn read(&self, path: &str) -> Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        self.check()?;
        match self {
            Self::Local => Ok(Box::new(
                tokio::fs::File::open(path)
                    .await
                    .map_err(|e| e.to_string())?,
            )),
            Self::Remote(c, _) => c.open_read_stream(path).await,
        }
    }
    pub async fn write_new(
        &self,
        path: &str,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        self.check()?;
        match self {
            Self::Local => Ok(Box::new(
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .await
                    .map_err(|e| e.to_string())?,
            )),
            Self::Remote(c, _) => c.open_write_new(path).await,
        }
    }
    pub async fn create_file(&self, path: &str) -> Result<(), String> {
        self.write_new(path)
            .await?
            .shutdown()
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn symlink(&self, target: &str, link: &str) -> Result<(), String> {
        self.check()?;
        match self {
            Self::Remote(c, _) => c.symlink(target, link).await,
            Self::Local => {
                let target = PathBuf::from(target);
                let link = PathBuf::from(link);
                tokio::task::spawn_blocking(move || {
                    #[cfg(unix)]
                    let result = std::os::unix::fs::symlink(target, link);
                    #[cfg(windows)]
                    let result = if target.is_dir() {
                        std::os::windows::fs::symlink_dir(target, link)
                    } else {
                        std::os::windows::fs::symlink_file(target, link)
                    };
                    result.map_err(|e| e.to_string())
                })
                .await
                .map_err(|_| "link task failed")?
            }
        }
    }
    pub fn join(&self, root: &str, name: &str) -> String {
        match self {
            Self::Local => Path::new(root).join(name).to_string_lossy().into_owned(),
            Self::Remote(_, _) => format!("{}/{name}", root.trim_end_matches('/')),
        }
    }
}

pub(super) fn safe_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name.contains(['/', '\\', '\0', '\r', '\n', ':'])
    {
        return Err("unsafe file name".into());
    }
    #[cfg(windows)]
    {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        if name.ends_with(['.', ' '])
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err("file name is reserved on Windows".into());
        }
    }
    Ok(())
}
