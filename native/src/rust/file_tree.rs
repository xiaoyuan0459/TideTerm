//! Plan before mutation; stream through private staging files and check conflicts.
use super::file_endpoint::{safe_name, Endpoint, Metadata};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static STAGING: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
pub(super) struct Progress {
    pub cancel: Arc<AtomicBool>,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub bytes: AtomicU64,
    pub current: Mutex<String>,
}
impl Progress {
    pub fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("operation cancelled".into())
        } else {
            Ok(())
        }
    }
}
struct Entry {
    relative: Vec<String>,
    metadata: Metadata,
}
fn joined(endpoint: &Endpoint, root: &str, parts: &[String]) -> String {
    parts
        .iter()
        .fold(root.to_owned(), |path, name| endpoint.join(&path, name))
}

async fn plan(
    source: &Endpoint,
    root: &str,
    copying: bool,
    progress: &Progress,
) -> Result<Vec<Entry>, String> {
    let mut pending = vec![Vec::<String>::new()];
    let mut entries = Vec::new();
    let mut name_bytes = 0;
    while let Some(relative) = pending.pop() {
        progress.check()?;
        source.check()?;
        if relative.len() > 32 || entries.len() >= 20000 {
            return Err("recursive operation exceeds depth or entry budget".into());
        }
        let path = joined(source, root, &relative);
        let metadata = source
            .metadata(&path)
            .await?
            .ok_or("source no longer exists")?;
        if metadata.kind == 2 && copying {
            return Err("copying symbolic links is not supported; source was not modified".into());
        }
        if metadata.kind == 1 {
            for name in source.list(&path, progress.cancel.clone()).await? {
                name_bytes += name.len();
                if name_bytes > 4 * 1024 * 1024 || pending.len() + entries.len() >= 20000 {
                    return Err("recursive plan exceeds budget".into());
                }
                let mut child = relative.clone();
                child.push(name);
                pending.push(child);
            }
        }
        entries.push(Entry { relative, metadata });
    }
    progress
        .total
        .store(entries.len() as u64, Ordering::Relaxed);
    Ok(entries)
}

pub(super) async fn copy(
    source: &Endpoint,
    from: &str,
    target: &Endpoint,
    to: &str,
    policy: u8,
    progress: &Progress,
) -> Result<(), String> {
    copy_checked(source, from, target, to, policy, progress, None).await
}

pub(super) async fn copy_checked(
    source: &Endpoint,
    from: &str,
    target: &Endpoint,
    to: &str,
    policy: u8,
    progress: &Progress,
    expected: Option<&Metadata>,
) -> Result<(), String> {
    if policy > 2 {
        return Err("invalid conflict policy".into());
    }
    if matches!((source, target), (Endpoint::Local, Endpoint::Local)) {
        let canonical = tokio::fs::canonicalize(from)
            .await
            .map_err(|e| e.to_string())?;
        let destination = std::path::Path::new(to);
        let anchor = if destination.exists() {
            destination
        } else {
            destination.parent().ok_or("invalid destination")?
        };
        if tokio::fs::canonicalize(anchor)
            .await
            .map_err(|e| e.to_string())?
            .starts_with(&canonical)
        {
            return Err("destination must not be inside the source".into());
        }
    }
    let entries = plan(source, from, true, progress).await?;
    let mut previous = Vec::with_capacity(entries.len());
    // Conflicts and link destinations are rejected before creating any directories.
    for entry in &entries {
        progress.check()?;
        let path = joined(target, to, &entry.relative);
        let existing = target.metadata(&path).await?;
        if entry.relative.is_empty() && expected.is_some() && existing.as_ref() != expected {
            return Err("destination changed since editing began".into());
        }
        if let Some(meta) = &existing {
            if meta.kind != entry.metadata.kind || meta.kind == 2 {
                return Err("destination type conflict or symbolic link".into());
            }
            if meta.kind == 0 && policy == 0 {
                return Err("destination exists; choose skip or replace".into());
            }
        }
        previous.push(existing);
    }
    for (entry, previous) in entries.iter().zip(previous) {
        progress.check()?;
        source.check()?;
        target.check()?;
        let src = joined(source, from, &entry.relative);
        let dst = joined(target, to, &entry.relative);
        *progress.current.lock().unwrap() = entry.relative.last().cloned().unwrap_or_default();
        if entry.metadata.kind == 1 {
            if previous.is_none() {
                target.mkdir(&dst).await?;
            }
        } else if !(policy == 1 && previous.is_some()) {
            let suffix = STAGING.fetch_add(1, Ordering::Relaxed);
            let stage = format!(
                "{dst}.tideterm-{}-{}-{suffix}.part",
                std::process::id(),
                chrono::Utc::now().timestamp_micros()
            );
            let mut created = false;
            let result = async {
                let mut reader = source.read(&src).await?;
                let mut writer = target.write_new(&stage).await?;
                created = true;
                let mut buffer = vec![0; 65536];
                loop {
                    progress.check()?;
                    source.check()?;
                    target.check()?;
                    let count = reader.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if count == 0 {
                        break;
                    }
                    writer
                        .write_all(&buffer[..count])
                        .await
                        .map_err(|e| e.to_string())?;
                    progress.bytes.fetch_add(count as u64, Ordering::Relaxed);
                }
                writer.flush().await.map_err(|e| e.to_string())?;
                writer.shutdown().await.map_err(|e| e.to_string())?;
                drop(writer);
                drop(reader);
                progress.check()?;
                if source.metadata(&src).await? != Some(entry.metadata.clone())
                    || target.metadata(&dst).await? != previous
                {
                    return Err("source or destination changed during transfer".into());
                }
                if policy == 2 && previous.is_some() {
                    target.replace_file(&stage, &dst).await
                } else {
                    target.rename_new(&stage, &dst).await
                }
            }
            .await;
            if let Err(error) = result {
                if created {
                    let cleanup = match target.metadata(&stage).await {
                        Ok(None) => Ok(()),
                        Ok(Some(_)) => target.remove(&stage, 0).await,
                        Err(error) => Err(error),
                    };
                    if let Err(cleanup) = cleanup {
                        return Err(format!("{error}; temporary copy cleanup could not be confirmed at {stage}: {cleanup}"));
                    }
                }
                return Err(error);
            }
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

pub(super) async fn remove(
    source: &Endpoint,
    path: &str,
    progress: &Progress,
) -> Result<(), String> {
    let entries = plan(source, path, false, progress).await?;
    for entry in entries.into_iter().rev() {
        progress.check()?;
        let current = joined(source, path, &entry.relative);
        let meta = source
            .metadata(&current)
            .await?
            .ok_or("source changed during deletion")?;
        if meta.kind != entry.metadata.kind {
            return Err("source type changed during deletion".into());
        }
        // A newly created entry makes rmdir fail; never follow a replacement link.
        source.remove(&current, meta.kind).await?;
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

pub(super) fn validate_path(endpoint: &Endpoint, path: &str) -> Result<(), String> {
    if path.len() > 4096 || path.contains(['\0', '\r', '\n']) {
        return Err("invalid file path".into());
    }
    match endpoint {
        Endpoint::Local if !std::path::Path::new(path).is_absolute() => {
            return Err("local file path must be absolute".into())
        }
        Endpoint::Remote(_, _) if !path.starts_with('/') => {
            return Err("remote file path must be absolute".into())
        }
        _ => {}
    }
    for part in path
        .replace('\\', "/")
        .split('/')
        .skip(1)
        .filter(|s| !s.is_empty())
    {
        safe_name(part)?;
    }
    if path.ends_with(['/', '\\']) || std::path::Path::new(path).file_name().is_none() {
        return Err("refusing filesystem root operation".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "file_tree_tests.rs"]
mod tests;
