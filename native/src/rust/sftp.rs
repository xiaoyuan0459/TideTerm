//! SFTP 会话域（Phase C ⑤）—— channel 缓存 + 异步操作结果队列 + 传输队列。
//!
//! 架构（对齐 ④ 的事件驱动模式，UI 线程零 block_on）：
//! - `Protocol::open_sftp()` 按 uid 惰性开 channel（缓存复用）；
//! - 目录操作（list/mkdir/remove/rename）spawn 到共享 runtime，完成结果进
//!   [`OPS`] 队列——C++ 泵轮询 `take_ops()` 一次性领取；
//! - 上传/下载用 core 的可续传流式 helper（4MB chunk + 250ms 进度节流），进度直接
//!   更新 [`TRANSFERS`] 队列（版本号驱动 C++ 跳过无变化轮询）；
//! - 暂停/取消经传输控制状态在 chunk 边界生效。
//!
//! `sftp_model` 提供 SftpFileEntry/TransferItem 与人类可读格式化。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use tiderm_core::sftp::{SftpFileEntry, SftpTransport};

#[cfg(test)]
mod daily_live_tests;
mod lifecycle;
#[cfg(test)]
mod live_tests;
#[path = "shared/sftp.rs"]
pub mod sftp_model;
mod transfer_control;
mod transfer_index;
mod transfer_limit;
mod transfers;
pub use lifecycle::close_session;
use transfers::{bump, cancels, transfers, trim_completed};
pub use transfers::{
    cancel_transfer, pause_transfer, resume_transfer, sftp_download, sftp_upload,
    transfers_snapshot, transfers_version_value,
};

pub fn sftp_close(uid: &str) {
    close_session(uid);
}

pub fn session_ready(uid: &str) -> bool {
    crate::rust::term::protocol_of_uid(uid).is_some_and(|protocol| protocol.is_alive())
}

pub(super) fn workflow_endpoint(
    uid: &str,
) -> Result<(Arc<dyn SftpTransport>, Arc<AtomicBool>), String> {
    if !super::term::sftp_allowed(uid) {
        return Err("SFTP is disabled or the session closed".into());
    }
    let token = lifecycle::token(uid).ok_or("SFTP session closed")?;
    let channel = with_channel(uid, Arc::clone).ok_or("SFTP channel is not ready")?;
    Ok((channel, token))
}

// ════════════════════ channel 缓存 ════════════════════

fn channels() -> &'static Mutex<HashMap<String, Arc<dyn SftpTransport>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<dyn SftpTransport>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

type TransferPool = HashMap<String, Vec<Arc<dyn SftpTransport>>>;

fn transfer_pools() -> &'static Mutex<TransferPool> {
    static P: OnceLock<Mutex<TransferPool>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Default)]
struct PoolGoal {
    requested: usize,
    generation: u64,
    pending: usize,
}

fn pool_goals() -> &'static Mutex<HashMap<String, PoolGoal>> {
    static GOALS: OnceLock<Mutex<HashMap<String, PoolGoal>>> = OnceLock::new();
    GOALS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct PoolSlots {
    capacity: Arc<Semaphore>,
    available: Mutex<VecDeque<Arc<dyn SftpTransport>>>,
}

impl PoolSlots {
    fn empty() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(0)),
            available: Mutex::new(VecDeque::new()),
        }
    }

    fn add(&self, channels: &[Arc<dyn SftpTransport>]) {
        self.available
            .lock()
            .unwrap()
            .extend(channels.iter().cloned());
        self.capacity.add_permits(channels.len());
    }

    fn take(self: &Arc<Self>, permit: OwnedSemaphorePermit) -> Option<TransferSlot> {
        let channel = self.available.lock().unwrap().pop_front()?;
        Some(TransferSlot {
            pool: self.clone(),
            channel: Some(channel),
            _permit: permit,
        })
    }
}

struct TransferSlot {
    pool: Arc<PoolSlots>,
    channel: Option<Arc<dyn SftpTransport>>,
    _permit: OwnedSemaphorePermit,
}

impl TransferSlot {
    fn channel(&self) -> &dyn SftpTransport {
        self.channel.as_ref().unwrap().as_ref()
    }
}

impl Drop for TransferSlot {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            self.pool.available.lock().unwrap().push_back(channel);
        }
    }
}

fn pool_capacity() -> &'static Mutex<HashMap<String, Arc<PoolSlots>>> {
    static CAPACITY: OnceLock<Mutex<HashMap<String, Arc<PoolSlots>>>> = OnceLock::new();
    CAPACITY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn capacity_for_session(uid: &str) -> Option<Arc<PoolSlots>> {
    pool_capacity().lock().unwrap().get(uid).cloned()
}

/// Grow in powers of two as queue demand appears. Reaching an idle queue drops
/// the extra connections and invalidates any late connection attempts.
fn request_workers(uid: &str, desired: usize) {
    if desired == 0 {
        return;
    }
    let target = desired.next_power_of_two().min(32);
    if pool_goals()
        .lock()
        .unwrap()
        .get(uid)
        .is_some_and(|goal| goal.requested >= target)
    {
        return;
    }
    let Some(token) = lifecycle::token(uid) else {
        return;
    };
    let Some(protocol) = crate::rust::term::protocol_of_uid(uid) else {
        return;
    };
    let (additional, generation) = {
        let mut goals = pool_goals().lock().unwrap();
        let goal = goals.entry(uid.to_string()).or_default();
        if goal.requested >= target {
            return;
        }
        let additional = target - goal.requested;
        goal.requested = target;
        goal.pending += 1;
        (additional, goal.generation)
    };
    let uid = uid.to_string();
    super::session::shared_runtime().spawn(async move {
        match protocol.open_sftp_workers(additional).await {
            Ok(workers) => lifecycle::extend_channels(&uid, &token, generation, workers),
            Err(error) => {
                tracing::warn!("SFTP worker setup: {error}");
                lifecycle::extend_channels(&uid, &token, generation, Vec::new());
            }
        }
    });
}

fn shrink_pool(uid: &str) {
    let mut goals = pool_goals().lock().unwrap();
    // A new file may have been queued after the finishing task saw zero demand.
    // Recheck while holding the goal lock used by request_workers.
    if transfer_index::desired(uid) != 0 {
        return;
    }
    if let Some(goal) = goals.get_mut(uid) {
        goal.requested = 0;
        goal.generation = goal.generation.wrapping_add(1);
        goal.pending = 0;
        if let Some(pool) = transfer_pools().lock().unwrap().get_mut(uid) {
            pool.clear();
        }
        let mut capacity = pool_capacity().lock().unwrap();
        if let Some(old) = capacity.insert(uid.to_string(), Arc::new(PoolSlots::empty())) {
            old.capacity.close();
        }
    }
}

fn cleanup_channel(uid: &str) -> Option<Arc<dyn SftpTransport>> {
    channels().lock().unwrap().get(uid).cloned()
}

/// 已缓存 channel（UI 侧状态显示用；None = 未开）。
pub fn channel_ready(uid: &str) -> bool {
    channels()
        .lock()
        .unwrap()
        .get(uid)
        .is_some_and(|channel| channel.is_alive())
}

mod open;
pub use open::sftp_open;

fn with_channel<R>(uid: &str, f: impl FnOnce(&Arc<dyn SftpTransport>) -> R) -> Option<R> {
    channels().lock().unwrap().get(uid).map(f)
}

// ════════════════════ 异步目录操作 ════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Open = 0,
    List = 1,
    Mkdir = 2,
    Remove = 3,
    Rename = 4,
    Transfer = 5,
}

/// 一次目录操作的完成结果（UI 泵 take_ops 领取）。
#[derive(Debug, Clone)]
pub struct SftpOp {
    pub kind: OpKind,
    pub uid: String,
    /// list = 列出的目录；其余 = 操作目标。
    pub path: String,
    pub ok: bool,
    pub error: String,
    pub entries: Vec<SftpFileEntry>,
}

fn failed_op(kind: OpKind, uid: &str, path: &str, error: &str) -> SftpOp {
    SftpOp {
        kind,
        uid: uid.to_string(),
        path: path.to_string(),
        ok: false,
        error: error.to_string(),
        entries: Vec::new(),
    }
}

fn push_op(op: SftpOp) {
    ops().lock().unwrap().push(op);
}

fn ops() -> &'static Mutex<Vec<SftpOp>> {
    static O: OnceLock<Mutex<Vec<SftpOp>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(Vec::new()))
}

/// 领取全部已完成操作结果（FIFO）。
pub fn take_ops() -> Vec<SftpOp> {
    std::mem::take(&mut *ops().lock().unwrap())
}

/// spawn 一个需要 channel 的操作；无 channel 时立即回失败结果。
fn spawn_with_channel<F, Fut>(uid: &str, path: &str, kind: OpKind, f: F)
where
    F: FnOnce(Arc<dyn SftpTransport>, String, String, Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let uid = uid.to_string();
    let path = path.to_string();
    let Some(token) = lifecycle::token(&uid) else {
        push_op(failed_op(kind, &uid, &path, "SFTP 会话已断开"));
        return;
    };
    let Some(channel) = with_channel(&uid, |c| c.clone()) else {
        push_op(failed_op(
            kind,
            &uid,
            &path,
            "SFTP 通道未打开（仅 SSH 会话支持）",
        ));
        return;
    };
    super::session::shared_runtime().spawn(async move {
        f(channel, uid, path, token).await;
    });
}

pub fn sftp_list(uid: &str, path: &str) {
    let show_hidden = super::term::session_options_of_uid(uid).is_none_or(|o| o.sftp_show_hidden);
    spawn_with_channel(
        uid,
        path,
        OpKind::List,
        move |sftp, uid, path, token| async move {
            let (ok, error, entries) = match sftp.list_dir(&path).await {
                Ok(e) => (
                    true,
                    String::new(),
                    e.into_iter()
                        .filter(|entry| show_hidden || !entry.name.starts_with('.'))
                        .collect(),
                ),
                Err(e) => (false, e, Vec::new()),
            };
            lifecycle::push_if_current(
                &token,
                SftpOp {
                    kind: OpKind::List,
                    uid,
                    path,
                    ok,
                    error,
                    entries,
                },
            );
        },
    );
}

pub fn sftp_mkdir(uid: &str, path: &str) {
    spawn_with_channel(
        uid,
        path,
        OpKind::Mkdir,
        |sftp, uid, path, token| async move {
            let e = sftp.mkdir(&path).await.err().unwrap_or_default();
            lifecycle::push_if_current(
                &token,
                SftpOp {
                    kind: OpKind::Mkdir,
                    uid,
                    path,
                    ok: e.is_empty(),
                    error: e,
                    entries: Vec::new(),
                },
            );
        },
    );
}

pub fn sftp_remove(uid: &str, path: &str) {
    spawn_with_channel(
        uid,
        path,
        OpKind::Remove,
        |sftp, uid, path, token| async move {
            // 与 Slint 壳同语义：先按文件删，失败再按目录删。
            let e = match sftp.remove_file(&path).await {
                Ok(()) => String::new(),
                Err(file_err) => match sftp.remove_dir(&path).await {
                    Ok(()) => String::new(),
                    Err(_) => file_err,
                },
            };
            lifecycle::push_if_current(
                &token,
                SftpOp {
                    kind: OpKind::Remove,
                    uid,
                    path,
                    ok: e.is_empty(),
                    error: e,
                    entries: Vec::new(),
                },
            );
        },
    );
}

pub fn sftp_rename(uid: &str, from: &str, to: &str) {
    let to = to.to_string();
    spawn_with_channel(
        uid,
        from,
        OpKind::Rename,
        move |sftp, uid, path, token| async move {
            let e = sftp.rename(&path, &to).await.err().unwrap_or_default();
            lifecycle::push_if_current(
                &token,
                SftpOp {
                    kind: OpKind::Rename,
                    uid,
                    path,
                    ok: e.is_empty(),
                    error: e,
                    entries: Vec::new(),
                },
            );
        },
    );
}

#[cfg(test)]
#[path = "sftp/pool_tests.rs"]
mod pool_tests;
#[cfg(test)]
#[path = "sftp/stress_tests.rs"]
mod stress_tests;
