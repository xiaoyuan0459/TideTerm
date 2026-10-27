//! Restartable SFTP transfers. Staging files survive pause and I/O failure.

use crate::sftp::{ProgressSink, SftpTransferProgress, SftpTransport, TransferDirection};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub const RUNNING: u8 = 0;
pub const PAUSED: u8 = 1;
pub const CANCELLED: u8 = 2;
const CHUNK: usize = 4 * 1024 * 1024;
const VERIFY_TAIL: u64 = 64 * 1024;

fn check(control: &AtomicU8) -> Result<(), String> {
    match control.load(Ordering::SeqCst) {
        PAUSED => Err("transfer paused".into()),
        CANCELLED => Err("transfer cancelled".into()),
        _ => Ok(()),
    }
}

struct ProgressReporter<'a> {
    sink: &'a dyn ProgressSink,
    session_id: &'a str,
    file_name: &'a str,
    direction: TransferDirection,
    total: u64,
    started: std::time::Instant,
    base: u64,
}

impl ProgressReporter<'_> {
    async fn emit(&self, done: u64) -> Result<(), String> {
        self.sink
            .progress(SftpTransferProgress {
                session_id: self.session_id.into(),
                file_name: self.file_name.into(),
                direction: self.direction.clone(),
                bytes_transferred: done,
                total_bytes: self.total,
                speed_bytes_per_sec: ((done - self.base) as f64
                    / self.started.elapsed().as_secs_f64().max(0.001))
                    as u64,
            })
            .await
    }
}

/// Resume an upload from `<remote>.part`; a cancelled upload removes staging.
pub async fn upload_resumable(
    sftp: &dyn SftpTransport,
    local: &Path,
    remote: &str,
    session_id: &str,
    sink: &dyn ProgressSink,
    control: &AtomicU8,
) -> Result<(), String> {
    let part = format!("{remote}.part");
    let result = upload_part(sftp, local, &part, session_id, sink, control).await;
    if control.load(Ordering::SeqCst) == CANCELLED {
        let _ = sftp.remove_file(&part).await;
        return Err("transfer cancelled".into());
    }
    result?;
    sftp.rename(&part, remote).await
}

async fn upload_part(
    sftp: &dyn SftpTransport,
    local: &Path,
    part: &str,
    session_id: &str,
    sink: &dyn ProgressSink,
    control: &AtomicU8,
) -> Result<(), String> {
    check(control)?;
    let metadata = tokio::fs::metadata(local)
        .await
        .map_err(|e| format!("read local metadata: {e}"))?;
    let total = metadata.len();
    let modified = metadata.modified().ok();
    let offset = sftp.stat(part).await.map(|s| s.size_bytes).unwrap_or(0);
    if offset > total {
        return Err("remote staging file exceeds local source size".into());
    }
    let mut source = tokio::fs::File::open(local)
        .await
        .map_err(|e| format!("open local source: {e}"))?;
    if offset > 0 {
        if offset > VERIFY_TAIL {
            let mut expected = vec![0; VERIFY_TAIL as usize];
            source
                .read_exact(&mut expected)
                .await
                .map_err(|e| e.to_string())?;
            let mut remote_head = sftp.open_read_stream_from(part, 0).await?;
            let mut actual = vec![0; VERIFY_TAIL as usize];
            remote_head
                .read_exact(&mut actual)
                .await
                .map_err(|e| e.to_string())?;
            if expected != actual {
                return Err("remote staging file differs from local source".into());
            }
        }
        let tail = offset.min(VERIFY_TAIL);
        source
            .seek(std::io::SeekFrom::Start(offset - tail))
            .await
            .map_err(|e| e.to_string())?;
        let mut expected = vec![0; tail as usize];
        source
            .read_exact(&mut expected)
            .await
            .map_err(|e| e.to_string())?;
        let mut remote_tail = sftp.open_read_stream_from(part, offset - tail).await?;
        let mut actual = vec![0; tail as usize];
        remote_tail
            .read_exact(&mut actual)
            .await
            .map_err(|e| e.to_string())?;
        if expected != actual {
            return Err("remote staging file differs from local source".into());
        }
    }
    source
        .seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|e| e.to_string())?;
    let mut target = sftp.open_write_stream_at(part, offset).await?;
    let name = local
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload");
    let mut done = offset;
    let started = std::time::Instant::now();
    let reporter = ProgressReporter {
        sink,
        session_id,
        file_name: name,
        direction: TransferDirection::Upload,
        total,
        started,
        base: offset,
    };
    let mut last_emit = started - std::time::Duration::from_millis(250);
    let mut buffer = vec![0; (total - offset).min(CHUNK as u64).max(1) as usize];
    reporter.emit(done).await?;
    while done < total {
        check(control)?;
        let n = source.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("local source became shorter during upload".into());
        }
        target
            .write_all(&buffer[..n])
            .await
            .map_err(|e| e.to_string())?;
        done += n as u64;
        if last_emit.elapsed().as_millis() >= 250 || done == total {
            reporter.emit(done).await?;
            last_emit = std::time::Instant::now();
        }
    }
    target.flush().await.map_err(|e| e.to_string())?;
    drop(target);
    check(control)?;
    let final_metadata = tokio::fs::metadata(local)
        .await
        .map_err(|e| e.to_string())?;
    if final_metadata.len() != total
        || final_metadata.modified().ok() != modified
        || sftp.stat(part).await?.size_bytes != total
    {
        return Err("upload size changed during transfer".into());
    }
    Ok(())
}

/// Resume a download from `<local>.part`; a cancelled download removes staging.
pub async fn download_resumable(
    sftp: &dyn SftpTransport,
    remote: &str,
    local: &Path,
    session_id: &str,
    sink: &dyn ProgressSink,
    control: &AtomicU8,
) -> Result<(), String> {
    let mut part_name = local.as_os_str().to_os_string();
    part_name.push(".part");
    let part = std::path::PathBuf::from(part_name);
    let result = download_part(sftp, remote, &part, session_id, sink, control).await;
    if control.load(Ordering::SeqCst) == CANCELLED {
        let _ = tokio::fs::remove_file(&part).await;
        return Err("transfer cancelled".into());
    }
    result?;
    tokio::fs::rename(&part, local)
        .await
        .map_err(|e| format!("move downloaded file into place: {e}"))
}

async fn download_part(
    sftp: &dyn SftpTransport,
    remote: &str,
    part: &Path,
    session_id: &str,
    sink: &dyn ProgressSink,
    control: &AtomicU8,
) -> Result<(), String> {
    check(control)?;
    let remote_stat = sftp.stat(remote).await?;
    let total = remote_stat.size_bytes;
    let offset = tokio::fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if offset > total {
        return Err("local staging file exceeds remote source size".into());
    }
    if offset > 0 {
        if offset > VERIFY_TAIL {
            let mut local_head = tokio::fs::File::open(part)
                .await
                .map_err(|e| e.to_string())?;
            let mut expected = vec![0; VERIFY_TAIL as usize];
            local_head
                .read_exact(&mut expected)
                .await
                .map_err(|e| e.to_string())?;
            let mut remote_head = sftp.open_read_stream_from(remote, 0).await?;
            let mut actual = vec![0; VERIFY_TAIL as usize];
            remote_head
                .read_exact(&mut actual)
                .await
                .map_err(|e| e.to_string())?;
            if expected != actual {
                return Err("local staging file differs from remote source".into());
            }
        }
        let tail = offset.min(VERIFY_TAIL);
        let mut local_tail = tokio::fs::File::open(part)
            .await
            .map_err(|e| e.to_string())?;
        local_tail
            .seek(std::io::SeekFrom::Start(offset - tail))
            .await
            .map_err(|e| e.to_string())?;
        let mut expected = vec![0; tail as usize];
        local_tail
            .read_exact(&mut expected)
            .await
            .map_err(|e| e.to_string())?;
        let mut remote_tail = sftp.open_read_stream_from(remote, offset - tail).await?;
        let mut actual = vec![0; tail as usize];
        remote_tail
            .read_exact(&mut actual)
            .await
            .map_err(|e| e.to_string())?;
        if expected != actual {
            return Err("local staging file differs from remote source".into());
        }
    }
    let mut source = sftp.open_read_stream_from(remote, offset).await?;
    let mut target = tokio::fs::OpenOptions::new()
        .create(offset == 0)
        .write(true)
        .truncate(offset == 0)
        .open(part)
        .await
        .map_err(|e| e.to_string())?;
    target
        .seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|e| e.to_string())?;
    let name = remote.rsplit('/').next().unwrap_or("download");
    let mut done = offset;
    let started = std::time::Instant::now();
    let reporter = ProgressReporter {
        sink,
        session_id,
        file_name: name,
        direction: TransferDirection::Download,
        total,
        started,
        base: offset,
    };
    let mut last_emit = started - std::time::Duration::from_millis(250);
    let mut buffer = vec![0; (total - offset).min(CHUNK as u64).max(1) as usize];
    reporter.emit(done).await?;
    while done < total {
        check(control)?;
        let requested = ((total - done) as usize).min(buffer.len());
        let n = source
            .read(&mut buffer[..requested])
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("remote source became shorter during download".into());
        }
        target
            .write_all(&buffer[..n])
            .await
            .map_err(|e| e.to_string())?;
        // Tokio's File may still have a background write in flight. Commit each
        // completed chunk before reporting a resumable offset to the UI.
        target.flush().await.map_err(|e| e.to_string())?;
        done += n as u64;
        if last_emit.elapsed().as_millis() >= 250 || done == total {
            reporter.emit(done).await?;
            last_emit = std::time::Instant::now();
        }
    }
    target.flush().await.map_err(|e| e.to_string())?;
    check(control)?;
    let final_stat = sftp.stat(remote).await?;
    if final_stat.size_bytes != total || final_stat.modified_unix != remote_stat.modified_unix {
        return Err("remote source size changed during download".into());
    }
    Ok(())
}
