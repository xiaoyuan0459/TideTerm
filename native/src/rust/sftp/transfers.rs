//! Transfer queue, control and progress reporting.

use super::{
    channel_ready, cleanup_channel, sftp_model, transfer_control::TransferControl, transfer_index,
    transfer_limit,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tiderm_core::sftp::{ProgressSink, SftpTransferProgress};
use tiderm_core::sftp_resume::{download_resumable, upload_resumable, CANCELLED, PAUSED};

// ════════════════════ 传输队列 ════════════════════

use sftp_model::{TransferDirection, TransferItem};

pub(super) fn transfers() -> &'static Mutex<Vec<TransferItem>> {
    static T: OnceLock<Mutex<Vec<TransferItem>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Vec::new()))
}

pub(super) fn trim_completed(q: &mut Vec<TransferItem>, limit: usize) {
    let mut remove = q
        .iter()
        .filter(|item| item.completed)
        .count()
        .saturating_sub(limit);
    q.retain(|item| {
        if item.completed && remove > 0 {
            remove -= 1;
            false
        } else {
            true
        }
    });
    transfer_index::rebuild(q);
}

fn transfers_version() -> &'static AtomicU64 {
    static V: OnceLock<AtomicU64> = OnceLock::new();
    V.get_or_init(|| AtomicU64::new(0))
}

pub(super) fn cancels() -> &'static Mutex<HashMap<String, Arc<TransferControl>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<TransferControl>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 传输队列快照（UI 泵按版本变化拉取）。
pub fn transfers_snapshot() -> Vec<TransferItem> {
    transfers().lock().unwrap().clone()
}

pub fn transfers_version_value() -> u64 {
    transfers_version().load(Ordering::Relaxed)
}

/// 取消传输（chunk 边界生效）。返回是否存在该传输。
pub fn cancel_transfer(id: &str) -> bool {
    if let Some(c) = cancels().lock().unwrap().get(id) {
        c.set(CANCELLED);
        return true;
    }
    false
}

/// Pause at the next chunk boundary, retaining the staging file.
pub fn pause_transfer(id: &str) -> bool {
    if let Some(control) = cancels().lock().unwrap().get(id) {
        control.set(PAUSED);
        return true;
    }
    false
}

/// Restart a paused or failed transfer from its verified staging offset.
pub fn resume_transfer(id: &str) -> bool {
    let item = {
        let mut queue = transfers().lock().unwrap();
        let Some(index) = transfer_index::position(&queue, id)
            .filter(|&index| queue[index].completed && queue[index].error.is_some())
        else {
            return false;
        };
        if cancels().lock().unwrap().contains_key(id) {
            return false;
        }
        let candidate = queue[index].clone();
        if !transfer_index::reserve(&candidate) {
            return false;
        }
        if !channel_ready(&candidate.session_id) {
            transfer_index::release(&candidate);
            return false;
        }
        queue[index].completed = false;
        queue[index].error = None;
        queue[index].clone()
    };
    bump();
    super::request_workers(&item.session_id, transfer_index::desired(&item.session_id));
    run_transfer(item);
    true
}

pub(super) fn bump() {
    transfers_version().fetch_add(1, Ordering::Relaxed);
}

fn transfer_id(prefix: &str, uid: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{uid}-{t}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// 进度回写：按 id 更新队列条目（完成/失败时终态一并落）。
struct QueueSink {
    id: String,
}

#[async_trait::async_trait]
impl ProgressSink for QueueSink {
    async fn progress(&self, p: SftpTransferProgress) -> Result<(), String> {
        let mut q = transfers().lock().unwrap();
        let mut changed = false;
        if let Some(t) = transfer_index::position(&q, &self.id)
            .and_then(|index| q.get_mut(index))
            .filter(|item| !item.completed)
        {
            t.bytes_transferred = p.bytes_transferred;
            t.total_bytes = p.total_bytes;
            changed = true;
        }
        drop(q);
        if changed {
            bump();
        }
        Ok(())
    }
}

fn spawn_transfer(uid: &str, local_path: String, remote_path: String, up: bool, size_hint: u64) {
    let id = transfer_id(if up { "up" } else { "down" }, uid);
    let file_name = if up {
        std::path::Path::new(&local_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
            .to_string()
    } else {
        remote_path
            .rsplit('/')
            .next()
            .unwrap_or("download")
            .to_string()
    };
    let mut item = TransferItem {
        id: id.clone(),
        session_id: uid.to_string(),
        file_name,
        direction: if up {
            TransferDirection::Upload
        } else {
            TransferDirection::Download
        },
        bytes_transferred: 0,
        total_bytes: size_hint,
        completed: false,
        error: None,
        local_path: local_path.clone(),
        remote_path: remote_path.clone(),
    };
    {
        let mut queue = transfers().lock().unwrap();
        if !transfer_index::reserve(&item) {
            item.completed = true;
            item.error = Some("同一目标已有传输任务".into());
        }
        let blocked = item.completed;
        transfer_index::push(&item, queue.len());
        queue.push(item.clone());
        if blocked {
            drop(queue);
            bump();
            return;
        }
    }
    bump();
    if !channel_ready(uid) {
        let mut q = transfers().lock().unwrap();
        transfer_index::release(&item);
        if let Some(t) = transfer_index::position(&q, &id).and_then(|index| q.get_mut(index)) {
            t.completed = true;
            t.error = Some("SFTP 通道未打开（仅 SSH 会话支持）".into());
        }
        trim_completed(&mut q, 200);
        bump();
        return;
    }
    super::request_workers(uid, transfer_index::desired(uid));
    run_transfer(item);
}

fn run_transfer(item: TransferItem) {
    let id = item.id.clone();
    let Some(capacity) = super::capacity_for_session(&item.session_id) else {
        finish_transfer(&id, Some("SFTP session closed".into()));
        return;
    };
    let control = Arc::new(TransferControl::new());
    cancels()
        .lock()
        .unwrap()
        .insert(id.clone(), control.clone());
    let limit = transfer_limit::for_session(&item.session_id);
    let options = crate::rust::term::session_options_of_uid(&item.session_id).unwrap_or_default();
    crate::rust::session::shared_runtime().spawn(async move {
        let permit = tokio::select! {
            result = async {
                let memory = limit.acquire_many_owned(transfer_limit::weight(item.total_bytes)).await?;
                let connection = capacity.capacity.clone().acquire_owned().await?;
                Ok::<_, tokio::sync::AcquireError>((connection, memory))
            } => result,
            _ = control.wait_stopped() => {
                let paused = control.state.load(Ordering::SeqCst) == PAUSED;
                if !paused {
                    if matches!(item.direction, TransferDirection::Upload) {
                        if let Some(channel) = cleanup_channel(&item.session_id) {
                            let _ = channel.remove_file(&format!("{}.part", item.remote_path)).await;
                        }
                    } else {
                        let _ = tokio::fs::remove_file(format!("{}.part", item.local_path)).await;
                    }
                }
                finish_transfer(&id, Some(if paused { "transfer paused".into() } else { "transfer cancelled".into() }));
                return;
            }
        };
        let Ok((connection, _memory)) = permit else {
            finish_transfer(&id, Some("SFTP session closed".into()));
            return;
        };
        let Some(slot) = capacity.take(connection) else {
            finish_transfer(&id, Some("SFTP session closed".into()));
            return;
        };
        let sink = QueueSink { id: id.clone() };
        let local = std::path::PathBuf::from(&item.local_path);
        let upload = matches!(item.direction, TransferDirection::Upload);
        let remote_metadata = if !upload && (options.sftp_preserve_modified || options.sftp_preserve_accessed) {
            match slot.channel().stat(&item.remote_path).await {
                Ok(value) => Some(value),
                Err(error) => { finish_transfer(&id, Some(error)); return; }
            }
        } else { None };
        let result = if upload {
            upload_resumable(
                slot.channel(),
                &local,
                &item.remote_path,
                &item.session_id,
                &sink,
                &control.state,
            )
            .await
        } else {
            download_resumable(
                slot.channel(),
                &item.remote_path,
                &local,
                &item.session_id,
                &sink,
                &control.state,
            )
            .await
        };
        let result = match result {
            Ok(()) if upload => tiderm_core::sftp_policy::preserve_upload_times(slot.channel(), &local, &item.remote_path, &options).await,
            Ok(()) => if let Some(metadata) = remote_metadata {
                tiderm_core::sftp_policy::preserve_download_times(&local, &metadata, &options).await
            } else { Ok(()) },
            Err(error) => Err(error),
        };
        finish_transfer(&id, result.err());
    });
}

/// 上传本地文件到远程目录（流式 4MB chunk）。
pub fn sftp_upload(uid: &str, local_path: &str, remote_dir: &str) {
    let remote = format!(
        "{}/{}",
        remote_dir.trim_end_matches('/'),
        std::path::Path::new(local_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
    );
    let size = std::fs::metadata(local_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    spawn_transfer(uid, local_path.to_string(), remote, true, size);
}

/// 下载远程文件到本地目录（流式）。
pub fn sftp_download(uid: &str, remote_path: &str, local_dir: &str, size_bytes: u64) {
    let name = remote_path.rsplit('/').next().unwrap_or("download");
    let repair = crate::rust::term::session_options_of_uid(uid)
        .unwrap_or_default()
        .sftp_repair_names;
    let name = match tiderm_core::sftp_policy::local_file_name(name, repair) {
        Ok(name) => name,
        Err(error) => {
            super::push_op(super::failed_op(
                super::OpKind::Transfer,
                uid,
                remote_path,
                &error,
            ));
            return;
        }
    };
    let local = std::path::Path::new(local_dir)
        .join(name)
        .to_string_lossy()
        .into_owned();
    spawn_transfer(uid, local, remote_path.to_string(), false, size_bytes);
}

fn finish_transfer(id: &str, err: Option<String>) {
    let mut q = transfers().lock().unwrap();
    let session_id = transfer_index::release_id(id);
    if let Some(t) = transfer_index::position(&q, id).and_then(|index| q.get_mut(index)) {
        t.completed = true;
        if t.error.is_none() {
            t.error = err;
        }
        // 总量兜底（stat 失败的传输至少显示已传量）。
        if t.total_bytes == 0 {
            t.total_bytes = t.bytes_transferred;
        }
    }
    cancels().lock().unwrap().remove(id);
    static FINISHED: AtomicU64 = AtomicU64::new(0);
    if FINISHED.fetch_add(1, Ordering::Relaxed) % 50 == 49 {
        trim_completed(&mut q, 200);
    }
    let idle_session = session_id.filter(|uid| transfer_index::desired(uid) == 0);
    drop(q);
    if let Some(uid) = idle_session {
        super::shrink_pool(&uid);
    }
    bump();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_download_keeps_destination_until_worker_finishes_after_row_trim() {
        let directory = tempfile::tempdir().unwrap();
        let first = TransferItem {
            id: "download-disconnect-owner".into(),
            session_id: "download-disconnect-a".into(),
            file_name: "file".into(),
            direction: TransferDirection::Download,
            bytes_transferred: 0,
            total_bytes: 1,
            completed: false,
            error: None,
            local_path: directory.path().join("file").to_string_lossy().into_owned(),
            remote_path: "/file".into(),
        };
        let mut second = first.clone();
        second.id = "download-disconnect-contender".into();
        let control = Arc::new(TransferControl::new());
        cancels()
            .lock()
            .unwrap()
            .insert(first.id.clone(), control.clone());
        {
            let mut queue = transfers().lock().unwrap();
            assert!(transfer_index::reserve(&first));
            transfer_index::push(&first, queue.len());
            queue.push(first.clone());
        }
        super::super::close_session(&first.session_id);
        assert_eq!(control.state.load(Ordering::SeqCst), PAUSED);
        assert!(
            !transfer_index::reserve(&second),
            "disconnect precedes worker shutdown"
        );
        {
            let mut queue = transfers().lock().unwrap();
            queue.retain(|item| item.id != first.id);
            transfer_index::rebuild(&queue);
        }
        finish_transfer(&first.id, None);
        assert!(
            transfer_index::reserve(&second),
            "trimmed rows must not leak reservations"
        );
        transfer_index::release(&second);
    }
}
