//! Legacy streaming SFTP helpers retained for the shared transport API.

use crate::sftp::{ProgressSink, SftpTransferProgress, SftpTransport, TransferDirection};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Streaming transfers have no file size limit. Chunk size is 4 MB (Phase 1):
/// the vendored libssh2 raises the SFTP channel window to 16 MB, so a 4 MB
/// write fills the window in fewer calls, reducing per-write overhead. Memory
/// stays O(chunk_size).
const STREAM_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Progress event throttle interval (ms). Emit at most every 250 ms.
const PROGRESS_THROTTLE_MS: u64 = 250;

/// Choose a transfer buffer size that keeps memory proportional to the file.
/// Large files keep the full 4 MB streaming window; small files use a small
/// buffer so many-file transfers do not allocate 4 MB per file.
fn transfer_chunk_size(total_bytes: u64) -> usize {
    if total_bytes < 256 * 1024 {
        64 * 1024
    } else if total_bytes < 4 * 1024 * 1024 {
        256 * 1024
    } else {
        STREAM_CHUNK_SIZE
    }
}

/// Remote `.part` staging name for a transfer target: transfers stage into it
/// and rename it into place on success, so a cancelled or failed transfer
/// leaves no truncated file at the final path (R-07 / SFTP-08).
fn remote_part_path(path: &str) -> String {
    format!("{path}.part")
}

/// Local sibling of [`remote_part_path`] for downloads.
pub(crate) fn local_part_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".part");
    std::path::PathBuf::from(name)
}

/// Best-effort cleanup helper: a missing staging file is not an error here.
async fn remove_quietly(sftp: &dyn SftpTransport, path: &str) {
    let _ = sftp.remove_file(path).await;
}

/// Stream a local file to a remote SFTP path in chunks, reporting progress.
///
/// `cancel`, when provided, is polled at each chunk boundary; returning `true`
/// aborts the transfer with `"transfer cancelled"`.
///
/// The payload is staged at `<remote_path>.part` and renamed into place only
/// after a successful flush; cancellation or failure removes the staging file
/// so the remote side never keeps a truncated target.
pub async fn upload_stream(
    sftp: &dyn SftpTransport,
    local_path: &std::path::Path,
    remote_path: &str,
    session_id: &str,
    progress: &dyn ProgressSink,
    cancel: Option<&AtomicBool>,
) -> std::result::Result<(), String> {
    let part = remote_part_path(remote_path);
    match upload_to_part(sftp, local_path, &part, session_id, progress, cancel).await {
        Ok(()) => match sftp.rename(&part, remote_path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                remove_quietly(sftp, &part).await;
                Err(format!("move uploaded file into place: {e}"))
            }
        },
        Err(e) => {
            remove_quietly(sftp, &part).await;
            Err(e)
        }
    }
}

async fn upload_to_part(
    sftp: &dyn SftpTransport,
    local_path: &std::path::Path,
    part_remote: &str,
    session_id: &str,
    progress: &dyn ProgressSink,
    cancel: Option<&AtomicBool>,
) -> std::result::Result<(), String> {
    let total_size = tokio::fs::metadata(local_path)
        .await
        .map_err(|e| format!("read local file metadata: {e}"))?
        .len();

    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("open local file: {e}"))?;

    let mut remote_file = sftp.open_write_stream(part_remote).await?;

    let file_name = local_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let chunk_size = transfer_chunk_size(total_size);
    let mut buf = vec![0u8; chunk_size];
    let mut transferred: u64 = 0;
    let start = std::time::Instant::now();
    let mut last_emit = std::time::Instant::now();

    loop {
        if let Some(c) = cancel
            && c.load(Ordering::SeqCst)
        {
            return Err("transfer cancelled".to_string());
        }
        let n = local_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("read local file chunk: {e}"))?;
        if n == 0 {
            break;
        }
        remote_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("write remote file chunk: {e}"))?;
        transferred += n as u64;

        if last_emit.elapsed().as_millis() as u64 >= PROGRESS_THROTTLE_MS
            || transferred == total_size
        {
            let elapsed_secs = start.elapsed().as_secs_f64().max(0.001);
            let speed = (transferred as f64 / elapsed_secs) as u64;
            progress
                .progress(SftpTransferProgress {
                    session_id: session_id.to_string(),
                    file_name: file_name.clone(),
                    direction: TransferDirection::Upload,
                    bytes_transferred: transferred,
                    total_bytes: total_size,
                    speed_bytes_per_sec: speed,
                })
                .await?;
            last_emit = std::time::Instant::now();
        }
    }

    remote_file
        .flush()
        .await
        .map_err(|e| format!("flush remote file: {e}"))?;
    Ok(())
}

/// Stream a remote SFTP file to a local path in chunks, reporting progress.
///
/// `cancel`, when provided, is polled at each chunk boundary; returning `true`
/// aborts the transfer with `"transfer cancelled"`.
///
/// The payload is staged at `<local_path>.part` and renamed into place only
/// after a successful flush; cancellation or failure removes the staging file
/// so a re-run never resumes onto a truncated local target.
pub async fn download_stream(
    sftp: &dyn SftpTransport,
    remote_path: &str,
    local_path: &std::path::Path,
    session_id: &str,
    progress: &dyn ProgressSink,
    cancel: Option<&AtomicBool>,
) -> std::result::Result<(), String> {
    let part = local_part_path(local_path);
    match download_to_part(sftp, remote_path, &part, session_id, progress, cancel).await {
        Ok(()) => tokio::fs::rename(&part, local_path).await.map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("move downloaded file into place: {e}")
        }),
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

async fn download_to_part(
    sftp: &dyn SftpTransport,
    remote_path: &str,
    part_local: &std::path::Path,
    session_id: &str,
    progress: &dyn ProgressSink,
    cancel: Option<&AtomicBool>,
) -> std::result::Result<(), String> {
    let total_size = sftp.stat(remote_path).await?.size_bytes;

    let mut remote_file = sftp.open_read_stream(remote_path).await?;

    let mut local_file = tokio::fs::File::create(part_local)
        .await
        .map_err(|e| format!("create local file: {e}"))?;

    let file_name = remote_path
        .split('/')
        .next_back()
        .unwrap_or("unknown")
        .to_string();

    let chunk_size = transfer_chunk_size(total_size);
    let mut buf = vec![0u8; chunk_size];
    let mut transferred: u64 = 0;
    let start = std::time::Instant::now();
    let mut last_emit = std::time::Instant::now();

    loop {
        if let Some(c) = cancel
            && c.load(Ordering::SeqCst)
        {
            return Err("transfer cancelled".to_string());
        }
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("read remote file chunk: {e}"))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("write local file chunk: {e}"))?;
        transferred += n as u64;

        if last_emit.elapsed().as_millis() as u64 >= PROGRESS_THROTTLE_MS
            || transferred == total_size
        {
            let elapsed_secs = start.elapsed().as_secs_f64().max(0.001);
            let speed = (transferred as f64 / elapsed_secs) as u64;
            progress
                .progress(SftpTransferProgress {
                    session_id: session_id.to_string(),
                    file_name: file_name.clone(),
                    direction: TransferDirection::Download,
                    bytes_transferred: transferred,
                    total_bytes: total_size,
                    speed_bytes_per_sec: speed,
                })
                .await?;
            last_emit = std::time::Instant::now();
        }
    }

    local_file
        .flush()
        .await
        .map_err(|e| format!("flush local file: {e}"))?;
    Ok(())
}
