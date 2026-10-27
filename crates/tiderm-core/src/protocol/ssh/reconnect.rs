//! SSH reconnection and background read thread.

use crate::error::{Error, Result};
use crate::protocol::ssh::config::SshAuth;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn require_known_host(
    status: crate::known_hosts::KnownHostStatus,
    host: &str,
    port: u16,
) -> Result<()> {
    use crate::known_hosts::KnownHostStatus;
    match status {
        KnownHostStatus::Known => Ok(()),
        KnownHostStatus::Unknown => Err(Error::Ssh(format!(
            "reconnect refused unknown host key for {host}:{port}"
        ))),
        KnownHostStatus::KeyChanged => Err(Error::Ssh(format!(
            "reconnect refused changed host key for {host}:{port}"
        ))),
    }
}

fn verify_reconnect_host_key(session: &ssh2::Session, host: &str, port: u16) -> Result<()> {
    let (key, _) = session
        .host_key()
        .ok_or_else(|| Error::Ssh("reconnect server did not present a host key".into()))?;
    let status = crate::known_hosts::verify_status(host, port, key)?;
    require_known_host(status, host, port)
}
use tokio::sync::mpsc;

use super::connection::{connect_session, run_bounded, SshInstance};

/// 静默链路看门狗的上限（R-16）。
/// 超过它仍"敲了键却一个字节都没回来"才判死 —— 健康终端的回显是 ms 级
/// （本地 PTY 731 µs / SSH 壳层路径在 R-21 修好后同量级），给 120 s 是极端宽松的余量；
/// 反过来，`limit == 0` 时整个看门狗关闭（用户可自行关掉）。
pub const LINK_WATCHDOG_MAX_SECS: u64 = 600;

/// **R-16 看门狗判据**：什么情况下"read 一直超时"才该算链路已死。
///
/// 三条都满足才判死：
/// 1. `limit > 0` —— 用户没关看门狗；
/// 2. `written > echoed_writes` —— **有键入却没有任何回字节**（空闲会话不算死，
///    否则挂一下午不敲键的健康会话会被误杀 —— 这正是 R-16 不能简单用"多久没数据"的原因）；
/// 3. `silent_for >= limit` —— 沉默已超过阈值。
///
/// 健康终端的回显是 ms 级（本地 PTY 实测 731 µs；R-21 修好后 SSH 路径同量级），
/// 默认 120 s 是极端宽松的余量。单独抽成纯函数是为了把**误判面**单测掉。
#[inline]
pub fn link_watchdog_should_die(
    written: usize,
    echoed_writes: usize,
    silent_for: Duration,
    limit: Duration,
) -> bool {
    !limit.is_zero() && written > echoed_writes && silent_for >= limit
}

/// Spawn a background thread that reads from the PTY channel and forwards
/// output to the mpsc. Exits on EOF or a non-transient error.
///
/// `written` 是本会话累计写出的字节数（键盘输入）；`watchdog_secs` 启用**静默链路看门狗**
/// （R-16）："有键入却迟迟没有任何字节回来"超过阈值即判死。
/// Active keepalive uses ssh2 0.9.6; the independent echo watchdog detects stalled input.
pub struct ReaderProbe {
    pub watchdog_secs: u64,
    pub keepalive_secs: u32,
    pub session: ssh2::Session,
    pub blocking_mode_gate: Arc<std::sync::Mutex<()>>,
}

pub fn spawn_read_task(
    mut channel: super::raw_channel::Channel,
    tx: mpsc::Sender<Vec<u8>>,
    alive: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    exit_code: Arc<std::sync::Mutex<Option<u32>>>,
    written: Arc<std::sync::atomic::AtomicUsize>,
    probe: ReaderProbe,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        // R-16 看门狗状态：`echoed` = 上一次收到字节时已写出的量；只有"又有新写入"
        // 且超过 watchdog 仍无字节，才说明链路静默死亡（空闲无键入的会话永不受影响）。
        let watchdog_after = Duration::from_secs(probe.watchdog_secs.min(LINK_WATCHDOG_MAX_SECS));
        let mut last_echo = Instant::now();
        let mut echoed_writes = written.load(Ordering::SeqCst);
        let mut next_keepalive = Instant::now();
        loop {
            if !alive.load(Ordering::SeqCst) || !active.load(Ordering::SeqCst) {
                break;
            }
            let result = {
                // SFTP temporarily changes the shared session's blocking mode.
                // Skip this read rather than stealing a blocking empty PTY read.
                let _mode = match probe.blocking_mode_gate.try_lock() {
                    Ok(mode) => mode,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(super::connection::READ_POLL_MS));
                        continue;
                    }
                    Err(std::sync::TryLockError::Poisoned(_)) => break,
                };
                if probe.keepalive_secs > 0 && Instant::now() >= next_keepalive {
                    match probe.session.keepalive_send() {
                        Ok(next) => {
                            next_keepalive =
                                Instant::now() + Duration::from_secs(next.max(1) as u64)
                        }
                        Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                            next_keepalive = Instant::now() + Duration::from_millis(100)
                        }
                        Err(_) => break,
                    }
                }
                channel.read(&mut buf)
            }; // Release the mode gate before forwarding output or sleeping.
            match result {
                Ok(0) => break, // EOF
                Ok(n) => {
                    echoed_writes = written.load(Ordering::SeqCst);
                    last_echo = Instant::now();
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // R-16：静默链路。空闲（无新键入）永远不算死；只有"敲了键却没有任何
                    // 回字节"持续超过阈值才判死 —— healthy 终端的回显是 ms 级。
                    if link_watchdog_should_die(
                        written.load(Ordering::SeqCst),
                        echoed_writes,
                        last_echo.elapsed(),
                        watchdog_after,
                    ) {
                        tracing::warn!(
                            target: "ssh",
                            "link watchdog: {}B written since last byte back, silent for {:?} — marking dead",
                            written.load(Ordering::SeqCst) - echoed_writes,
                            last_echo.elapsed()
                        );
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(super::connection::READ_POLL_MS));
                }
                Err(_) => break,
            }
        }
        // Best-effort exit status capture once the channel has ended.
        channel.stop_forwarding();
        if let Ok(code) = channel.exit_status()
            && let Ok(mut ec) = exit_code.lock()
        {
            *ec = Some(code as u32);
        }
        alive.store(false, Ordering::SeqCst);
    })
}

impl SshInstance {
    /// Attempt to re-establish the SSH connection.
    ///
    /// Only password and public-key sessions can reconnect automatically;
    /// keyboard-interactive requires frontend interaction and is rejected.
    pub async fn reconnect_internal(&self) -> Result<()> {
        if !self.config.reconnect_enabled {
            return Err(Error::Ssh("auto-reconnect disabled".into()));
        }

        if matches!(self.config.auth, SshAuth::KeyboardInteractive) {
            return Err(Error::Ssh(
                "auto-reconnect not supported for keyboard-interactive auth".into(),
            ));
        }
        if self.config.max_reconnect_attempts == 0 {
            return Err(Error::Ssh("auto-reconnect has zero attempts".into()));
        }

        for attempt in 1..=self.config.max_reconnect_attempts {
            if attempt > 1 {
                tokio::time::sleep(Duration::from_secs(self.config.reconnect_interval_secs)).await;
            }

            let cfg = self.config.clone();
            let cols = self.cols;
            let rows = self.rows;
            // R-08/R-20 同源修复：重连的每一跳（TCP/握手/认证/开 PTY）都受
            // `connect_timeout_secs` 预算约束，超时即掐断 socket 收回线程。
            let attempt_deadline =
                Instant::now() + Duration::from_secs(cfg.connect_timeout_secs.max(1));
            let result = match connect_session(&cfg).await {
                Err(e) => Err(e),
                Ok((session, cancel)) => {
                    // A changed or unknown key is a trust failure, not a transient
                    // network error. Abort immediately without further attempts.
                    verify_reconnect_host_key(&session, &cfg.host, cfg.port)?;
                    let outcome = async {
                        let s_auth = session.clone();
                        let a_cfg = cfg.clone();
                        run_bounded(
                            &cancel,
                            attempt_deadline.saturating_duration_since(Instant::now()),
                            "reconnect auth",
                            move || super::auth::authenticate(&s_auth, &a_cfg),
                        )
                        .await?;
                        let s_pty = session.clone();
                        let pty_term = cfg.term.clone();
                        let forwarding_options = cfg.options.clone();
                        run_bounded(
                            &cancel,
                            attempt_deadline.saturating_duration_since(Instant::now()),
                            "reconnect pty",
                            move || {
                                super::auth::open_pty_shell(
                                    &s_pty,
                                    cols,
                                    rows,
                                    &pty_term,
                                    &forwarding_options,
                                )
                            },
                        )
                        .await
                    }
                    .await;
                    drop(cancel);
                    match outcome {
                        Ok(channel) => Ok((session, channel)),
                        Err(e) => Err(e),
                    }
                }
            };

            match result {
                Ok((session, channel)) => {
                    session.set_timeout(super::connection::INTERACTIVE_TIMEOUT_MS);
                    // R-21：与 connect() 第 5 步同构 —— 交互循环非阻塞，
                    // 否则重连后回显又变回 ~2 s 一次。
                    session.set_blocking(false);
                    *self.session.lock().await = session;
                    *self.channel.lock().await = channel;
                    self.alive.store(true, Ordering::SeqCst);
                    if let Ok(mut at) = self.connected_at.lock() {
                        *at = Instant::now();
                    }
                    if let Ok(mut ec) = self.exit_code.lock() {
                        *ec = None;
                    }
                    if let Ok(mut tx) = self.keyboard_interactive_tx.lock() {
                        *tx = None;
                    }

                    // Replace the reader thread with one on the new channel.
                    let mut read_task_guard = self.read_task.lock().await;
                    if let Some(old_active) = self.read_active.lock().await.take() {
                        old_active.store(false, Ordering::SeqCst);
                    }
                    if let Some(old_task) = read_task_guard.take() {
                        // The old reader exits promptly once `active` is cleared.
                        drop(old_task);
                    }
                    let (new_tx, new_rx) = mpsc::channel::<Vec<u8>>(256);
                    let read_channel = self.channel.lock().await.clone();
                    let new_active = Arc::new(AtomicBool::new(true));
                    let watchdog = self.config.link_watchdog_secs;
                    let written = self.bytes_written.clone();
                    let new_task = spawn_read_task(
                        read_channel,
                        new_tx,
                        self.alive.clone(),
                        new_active.clone(),
                        self.exit_code.clone(),
                        written,
                        ReaderProbe {
                            watchdog_secs: watchdog,
                            keepalive_secs: self.config.options.keepalive_secs,
                            session: self.session.lock().await.clone(),
                            blocking_mode_gate: self.blocking_mode_gate.clone(),
                        },
                    );
                    *read_task_guard = Some(new_task);
                    *self.read_active.lock().await = Some(new_active);
                    drop(read_task_guard);

                    *self.output_rx.lock().await = new_rx;
                    return Ok(());
                }
                Err(e) => {
                    if attempt == self.config.max_reconnect_attempts {
                        return Err(e);
                    }
                }
            }
        }

        Err(Error::Ssh("auto-reconnect exhausted attempts".into()))
    }
}

/// Clean up background resources when the SSH session is dropped.
impl Drop for SshInstance {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut active) = self.read_active.try_lock()
            && let Some(flag) = active.take()
        {
            flag.store(false, Ordering::SeqCst);
        }
        // Detach the reader thread; it exits promptly once `active` is cleared.
        if let Ok(mut guard) = self.read_task.try_lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod trust_tests {
    use super::require_known_host;
    use crate::known_hosts::KnownHostStatus;

    #[test]
    fn reconnect_requires_an_already_trusted_key() {
        assert!(require_known_host(KnownHostStatus::Known, "test.invalid", 22).is_ok());
        let unknown = require_known_host(KnownHostStatus::Unknown, "test.invalid", 22)
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("unknown host key"));
        let changed = require_known_host(KnownHostStatus::KeyChanged, "test.invalid", 22)
            .unwrap_err()
            .to_string();
        assert!(changed.contains("changed host key"));
    }
}

#[cfg(test)]
mod watchdog_tests {
    use super::link_watchdog_should_die;
    use std::time::Duration;

    const LIMIT: Duration = Duration::from_secs(5);
    const OVER: Duration = Duration::from_secs(6);
    const UNDER: Duration = Duration::from_secs(4);

    /// 黑洞：敲了键、超过阈值、零回字节 → 判死。
    #[test]
    fn silent_blackhole_with_typing_dies() {
        assert!(link_watchdog_should_die(12, 0, OVER, LIMIT));
    }

    /// **最重要的假阳性面**：空闲（没敲键）永不判死 —— 挂一下午不动的健康会话。
    #[test]
    fn idle_session_never_dies() {
        assert!(!link_watchdog_should_die(
            0,
            0,
            Duration::from_secs(86_400),
            LIMIT
        ));
        assert!(
            !link_watchdog_should_die(5, 5, OVER, LIMIT),
            "写入都已被回显"
        );
    }

    /// 还没到阈值不判死（给慢链路留余量）。
    #[test]
    fn under_threshold_never_dies() {
        assert!(!link_watchdog_should_die(3, 0, UNDER, LIMIT));
    }

    /// 用户关掉看门狗（`link_watchdog_secs = 0`）→ 永不判死。
    #[test]
    fn disabled_never_dies() {
        assert!(!link_watchdog_should_die(
            9,
            0,
            Duration::from_secs(86_400),
            Duration::ZERO
        ));
    }

    /// 边界：刚好等于阈值即判死；少 1 ns 不判。
    #[test]
    fn boundary_is_inclusive() {
        assert!(link_watchdog_should_die(1, 0, LIMIT, LIMIT));
        assert!(!link_watchdog_should_die(
            1,
            0,
            LIMIT - Duration::from_nanos(1),
            LIMIT
        ));
    }

    /// 边写边回（健康会话）：written 追上 echoed 时立刻脱保。
    #[test]
    fn echoed_write_disarms() {
        assert!(!link_watchdog_should_die(10, 10, OVER, LIMIT));
    }
}
