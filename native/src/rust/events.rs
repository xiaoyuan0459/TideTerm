//! 全局事件枢纽（Phase C ④）—— BackendEventSink / 主机密钥 inquirer / kb
//! 挑战队列 / 远程连接交付队列 的进程级单例与 drain 路由。
//!
//! 从 `session.rs` 拆出（行数门禁）：枢纽面与宿主生命周期分离——term 的
//! `pump_all` 经 [`drain_events`] 拉事件按 uid 分发；C++ prompt 轮询经
//! [`take_prompt`] / [`answer_host_key`]。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::broadcast;

use tiderm_core::auth::{EventHostKeyInquirer, KbSyncBackend};
use tiderm_core::event::{BackendEvent, BackendEventSink, ChallengeField};
use tiderm_core::protocol::Protocol;

pub(crate) fn sink() -> &'static BackendEventSink {
    static S: OnceLock<BackendEventSink> = OnceLock::new();
    S.get_or_init(BackendEventSink::new)
}

pub(crate) fn host_key() -> &'static Arc<EventHostKeyInquirer> {
    static H: OnceLock<Arc<EventHostKeyInquirer>> = OnceLock::new();
    H.get_or_init(|| Arc::new(EventHostKeyInquirer::new(sink().clone())))
}

/// 事件总线丢弃计数（R-11）。`Lagged(n)` 意味着这 n 条事件被广播通道**永久丢弃** ——
/// 读循环只 emit 一次、不重发，所以字节既拿不回也补不了页。里面可能混着 `Output`
/// （屏幕缺行）和 `Exit`（会话已死但标签不提示），因此只能「告知 + 全量重绘」，
/// 绝不能再 `continue` 静默吞掉：静默的表现是标签还在、输出缺行、会话已死不报错。
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

/// 累计丢弃数（进程级、不清零）。生产路径不需要它 —— 用户要的提示已经写在会话
/// note 上了；`#[cfg(test)]` 只留给无头测试读数，免得 bin 构建报死代码。
#[cfg(test)]
pub fn dropped_events() -> u64 {
    DROPPED_EVENTS.load(Ordering::Relaxed)
}

/// 全局广播接收端（UI 线程 drain；Mutex 仅防重入，不在异步线程持有）。
fn event_rx() -> &'static Mutex<broadcast::Receiver<BackendEvent>> {
    static R: OnceLock<Mutex<broadcast::Receiver<BackendEvent>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(sink().subscribe()))
}

/// Subscribe before a transport can emit its initial output or connection error.
pub(crate) fn prepare_receiver() {
    let _ = event_rx();
}

/// kb 挑战（同步 emit 侧，B3）：libssh2 回调线程 → sink 事件。
pub(crate) struct SinkKbBackend;

impl KbSyncBackend for SinkKbBackend {
    fn emit(
        &self,
        session_id: &str,
        name: &str,
        instruction: &str,
        fields: &[ChallengeField],
    ) -> Result<(), String> {
        sink().emit(BackendEvent::KeyboardInteractive {
            id: session_id.to_string(),
            name: name.to_string(),
            instruction: instruction.to_string(),
            fields: fields.to_vec(),
        });
        Ok(())
    }
}

/// 远程连接交付队列：connect 任务（runtime 线程）→ UI 线程 TermState。
type DeliveryQueue = Mutex<Vec<(String, Arc<dyn Protocol>)>>;

fn delivery() -> &'static DeliveryQueue {
    static D: OnceLock<DeliveryQueue> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Vec::new()))
}

/// 待 UI 应答的交互提示（host key / keyboard-interactive），FIFO。
#[derive(Debug, Clone)]
pub enum PendingPrompt {
    /// 主机密钥确认（state：0=Known 1=Unknown 2=KeyChanged）。
    HostKey {
        uid: String,
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
        state: u8,
    },
    /// 键盘交互挑战（多字段，echo 控制掩码显示）。
    Kb {
        uid: String,
        name: String,
        instruction: String,
        prompts: Vec<(String, bool)>,
    },
}

fn pending_prompts() -> &'static Mutex<Vec<PendingPrompt>> {
    static P: OnceLock<Mutex<Vec<PendingPrompt>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Vec::new()))
}

/// 应答主机密钥确认（C++ 对话框回调）。UI 线程短 block_on（oneshot send 即返）。
pub fn answer_host_key(uid: &str, accept: bool) -> bool {
    super::session::shared_runtime().block_on(async move { host_key().answer(uid, accept).await })
}

/// 把一个 BackendEvent 路由到 pending 队列（Output/Error 由 term 侧直接路由）。
fn route_prompt(ev: BackendEvent) {
    let prompt = match ev {
        BackendEvent::HostKeyConfirm {
            id,
            host,
            port,
            key_type,
            fingerprint,
            key,
        } => {
            let state = match tiderm_core::known_hosts::verify_status(&host, port, &key) {
                Ok(tiderm_core::known_hosts::KnownHostStatus::Known) => 0,
                Ok(tiderm_core::known_hosts::KnownHostStatus::KeyChanged) => 2,
                _ => 1,
            };
            PendingPrompt::HostKey {
                uid: id,
                host,
                port,
                key_type,
                fingerprint,
                state,
            }
        }
        BackendEvent::KeyboardInteractive {
            id,
            name,
            instruction,
            fields,
        } => PendingPrompt::Kb {
            uid: id,
            name,
            instruction,
            prompts: fields.into_iter().map(|f| (f.prompt, f.echo)).collect(),
        },
        _ => return,
    };
    pending_prompts().lock().unwrap().push(prompt);
}

/// drain 事件流：每 `Output` 块回调 `(uid, bytes)`；交互挑战入 pending 队列；
/// `Error`/`Exit` 回调 `(uid, 状态文本, is_error)` —— is_error 区分错误与正常结束
/// （R-19：手动断开/会话退出不该被当成 ⚠ 异常）。返回**本次被丢弃的事件条数**
/// （= 接收端被读写循环甩开、字节已永久丢失的条数；0 表示本轮没丢）。
///
/// 调用方（`term::pump_all`）据此做全量重同步 —— 不要把返回值扔掉当"没看见"。
pub fn drain_events(
    mut on_data: impl FnMut(&str, &[u8]),
    mut on_status: impl FnMut(&str, &str, bool),
) -> u64 {
    let mut rx = event_rx().lock().unwrap();
    let mut dropped = 0;
    loop {
        match rx.try_recv() {
            Ok(BackendEvent::Output { id, data }) => {
                on_data(&id, &data);
            }
            Ok(BackendEvent::Error { id, message }) => {
                on_status(&id, &message, true);
            }
            Ok(BackendEvent::Exit { id, .. }) => {
                on_status(&id, "会话已结束", false);
            }
            Ok(ev) => {
                route_prompt(ev);
            }
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                dropped += n;
                DROPPED_EVENTS.fetch_add(n, Ordering::Relaxed);
                continue;
            }
            Err(_) => break,
        }
    }
    dropped
}

/// R-11 的重同步侧：`Lagged(n)` 只给条数、**不能归属到 uid**（广播通道不记录来源），
/// 所以无法只重绘"受害会话"。保守做法是**所有活着的会话都重绘**，并在还没有错误/退出
/// 文本的会话上留一条可见提示 —— 缺行本身就是故障，静默会让用户以为屏幕是全的。
///
/// 放在枢纽里：**要不要重绘**由事件面决定，而标签表是进程级共享的。
pub(crate) fn resync_sessions_after_dropped_events(dropped: u64) {
    if dropped == 0 {
        return;
    }
    let note = format!("输出丢帧 {dropped} 条：屏幕可能缺行");
    super::term::TABS.with(|c| {
        let mut m = c.borrow_mut();
        for st in m.values_mut() {
            st.redraw_pending = true;
            if st.note.borrow().is_empty() {
                st.error.set(true); // 丢帧告警按 ⚠ 处理
                *st.note.borrow_mut() = note.clone();
            }
        }
    });
}

/// 取走最早的一条待应答提示（C++ 对话框轮询）。None = 无待处理。
pub fn take_prompt() -> Option<PendingPrompt> {
    let mut q = pending_prompts().lock().unwrap();
    if q.is_empty() {
        None
    } else {
        Some(q.remove(0))
    }
}

/// 弹出全部已交付但未被 TermState 领取的协议（term poll 时调用）。
pub fn take_deliveries() -> Vec<(String, Arc<dyn Protocol>)> {
    std::mem::take(&mut *delivery().lock().unwrap())
}

/// 远程连接任务成功侧入队（`session.rs` 的 spawn_connect 调）。
pub(crate) fn push_delivery(
    uid: String,
    active: &std::sync::atomic::AtomicBool,
    proto: Arc<dyn Protocol>,
) -> Result<(), Arc<dyn Protocol>> {
    let mut queue = delivery().lock().unwrap();
    if !active.load(Ordering::SeqCst) {
        return Err(proto);
    }
    queue.push((uid, proto));
    Ok(())
}

/// 关标签时同步移走尚未交付的连接，避免 UI 停止泵送后遗留 socket。
pub(crate) fn discard_delivery(uid: &str) {
    let stale = {
        let mut queue = delivery().lock().unwrap();
        let mut stale = Vec::new();
        queue.retain(|(queued_uid, proto)| {
            if queued_uid == uid {
                stale.push(proto.clone());
                false
            } else {
                true
            }
        });
        stale
    };
    for proto in stale {
        super::session::shared_runtime().spawn(async move {
            let _ = proto.disconnect().await;
        });
    }
}

/// Route backend events and connect deliveries into live terminal panes.
pub(crate) fn pump_all() -> std::collections::HashSet<String> {
    let mut datas: Vec<(String, Vec<u8>)> = Vec::new();
    let mut statuses: Vec<(String, String, bool)> = Vec::new();
    let dropped = drain_events(
        |uid, data| datas.push((uid.to_string(), data.to_vec())),
        |uid, status, is_error| statuses.push((uid.to_string(), status.to_string(), is_error)),
    );
    let mut touched: std::collections::HashSet<String> = datas
        .iter()
        .map(|(uid, _)| uid.clone())
        .chain(statuses.iter().map(|(uid, ..)| uid.clone()))
        .collect();
    let mut dead_sftp = Vec::new();
    resync_sessions_after_dropped_events(dropped);
    super::term::TABS.with(|c| {
        let mut tabs = c.borrow_mut();
        for (uid, data) in datas {
            if let Some(st) = tabs.values_mut().find(|st| st.uid == uid) {
                st.accept_output(&data);
                let replies = st.pane.take_replies();
                if !replies.is_empty() {
                    st.host.write(&replies);
                }
            }
        }
        for (uid, status, is_error) in statuses {
            if let Some(st) = tabs.values_mut().find(|st| st.uid == uid) {
                let connected = st.host.connected();
                let alive = st.host.alive();
                let exit_code = st.host.protocol().and_then(|p| p.exit_code());
                let ssh = st.config.ssh.as_ref();
                let enabled = super::term_lifecycle::auto_retry_enabled(&st.config);
                let max = ssh.map_or(0, |s| s.max_reconnect_attempts);
                if super::term_lifecycle::should_retry(super::term_lifecycle::RetryInput {
                    enabled,
                    manual_stop: st.manual_stop,
                    attempts: st.auto_attempts,
                    max_attempts: max,
                    connected,
                    alive,
                    exit_code,
                    status: &status,
                }) {
                    if st.auto_retry_at.is_none() {
                        let delay = ssh.map_or(1, |s| s.reconnect_interval_secs.max(1)) as u64;
                        st.auto_retry_at =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(delay));
                    }
                    // 重连排程是状态文案不是错误 —— 角标走 connecting 而非 ⚠。
                    st.error.set(false);
                    *st.note.borrow_mut() =
                        format!("连接中断，等待自动重连 {}/{}", st.auto_attempts + 1, max);
                    st.redraw_pending = true;
                } else if enabled
                    && !st.manual_stop
                    && st.auto_attempts >= max
                    && ((connected && !alive) || (st.auto_attempts > 0 && !connected))
                {
                    st.error.set(true);
                    *st.note.borrow_mut() = format!("自动重连失败（{max} 次）：{status}");
                } else {
                    st.error.set(is_error);
                    *st.note.borrow_mut() = status;
                }
                if connected && !alive {
                    dead_sftp.push(uid.clone());
                }
            }
        }
    });
    for uid in dead_sftp {
        super::sftp::close_session(&uid);
    }
    for (uid, proto) in take_deliveries() {
        touched.insert(uid.clone());
        let mut pending = Some(proto);
        super::term::TABS.with(|c| {
            if let Some(st) = c.borrow_mut().values_mut().find(|st| st.uid == uid) {
                st.host.deliver(pending.take().expect("协议只交付一次"));
                if st.auto_attempts > 0 {
                    st.auto_attempts = 0;
                    st.auto_retry_at = None;
                    st.note.borrow_mut().clear();
                    st.error.set(false);
                }
            }
        });
        if let Some(proto) = pending {
            super::session::shared_runtime().spawn(async move {
                let _ = proto.disconnect().await;
            });
        }
    }
    let now = std::time::Instant::now();
    let due = super::term::TABS.with(|c| {
        let mut tabs = c.borrow_mut();
        tabs.iter_mut()
            .filter_map(|(idx, st)| {
                if st.auto_retry_at.is_some_and(|at| at <= now) {
                    st.auto_retry_at = None;
                    Some(*idx)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });
    for idx in due {
        super::term_lifecycle::term_auto_reconnect_tab(idx);
    }
    touched
}
