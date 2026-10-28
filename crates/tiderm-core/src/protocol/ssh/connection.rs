//! SSH connection establishment — handshake, PTY allocation, channel management.
//!
//! v0.10 (libssh2 migration): backed by the blocking `ssh2` crate. A dedicated
//! reader thread pumps channel output into an mpsc; writes/resize/SFTP run via
//! `spawn_blocking`.

use crate::auth::{HostKeyInquirer, KbSyncBackend};
use crate::error::{Error, Result};
use crate::known_hosts::KnownHostStatus;
use crate::protocol::ssh::config::SshConfig;
use crate::protocol::ssh::handler::{host_key_type_name, sha256_fingerprint};
use ssh2::KnownHostKeyFormat;
use std::io::Write;

use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

/// Timeout (ms) applied to every libssh2 call once the interactive loop is
/// running.
pub(crate) const INTERACTIVE_TIMEOUT_MS: u32 = 2000;

/// Poll interval (ms) for the reader thread between timed-out reads.
pub(crate) const READ_POLL_MS: u64 = 2;

/// Total budget (s) for a single channel write to make progress. If the socket
/// stalls (WouldBlock/TimedOut) beyond this, fail instead of hanging forever.
pub(crate) const WRITE_BUDGET_SECS: u64 = 30;

/// An SSH session backed by libssh2 (`ssh2` crate).
///
/// The session and PTY channel are shared via `Arc<Mutex<…>>` so a reconnect
/// can swap them out; clones handed to worker threads share the underlying
/// libssh2 session (internally synchronized by the crate).
pub struct SshInstance {
    /// libssh2 session.
    pub session: Arc<Mutex<ssh2::Session>>,
    /// Serializes calls which temporarily switch this session's blocking mode.
    pub(crate) blocking_mode_gate: Arc<std::sync::Mutex<()>>,
    /// PTY channel.
    pub channel: Arc<Mutex<super::raw_channel::Channel>>,
    /// Buffered output receiver (fed by background read thread).
    pub output_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Connection alive flag.
    pub alive: Arc<AtomicBool>,
    /// Process exit code (set when the channel reaches EOF).
    pub exit_code: Arc<std::sync::Mutex<Option<u32>>>,
    /// Sender used to forward keyboard-interactive responses.
    pub keyboard_interactive_tx:
        Arc<std::sync::Mutex<Option<std::sync::mpsc::Sender<Vec<String>>>>>,
    /// Connection configuration retained for reconnection attempts.
    pub config: SshConfig,
    /// Terminal width retained for reconnection attempts.
    pub cols: u16,
    /// Terminal height retained for reconnection attempts.
    pub rows: u16,
    /// Timestamp when the connection was (re-)established.
    pub connected_at: Arc<std::sync::Mutex<Instant>>,
    /// Handle to the background read thread.
    pub read_task: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Per-reader active flag; cleared when a reconnect replaces the reader.
    pub read_active: Mutex<Option<Arc<AtomicBool>>>,
    /// 累计写出的字节数（键盘输入）—— R-16 静默链路看门狗的武装信号。
    pub bytes_written: Arc<AtomicUsize>,
}

impl SshInstance {
    /// Connect to an SSH server, authenticate, and request a PTY shell.
    ///
    /// `inquirer` resolves unknown-host-key confirmations and `kb_backend`
    /// bridges the synchronous keyboard-interactive prompt (both de-shelled;
    /// the Tauri bridges emit to the webview, the native shell to its UI).
    ///
    /// 超时语义（R-08/R-20）：`connect_timeout_secs` 约束**整条**建连 ——
    /// TCP + 握手 + 认证 + 开 PTY。每段都经 [`run_bounded`]：超时即对底层
    /// socket 强制 `shutdown`，被放弃的 blocking 线程立刻拿到错误退出，
    /// 线程与 socket 都不滞留（libssh2 的会话超时拦不住静默主机的 banner
    /// 等待，见 `env_tests::timed_out_connect_releases_socket_and_thread` 的
    /// 修复前实测）。唯一例外：主机密钥确认是用户交互（自带 60 s 内层
    /// 超时），其等待时间不占用预算。
    pub async fn connect(
        config: SshConfig,
        cols: u16,
        rows: u16,
        session_id: String,
        inquirer: &dyn HostKeyInquirer,
        kb_backend: Arc<dyn KbSyncBackend>,
    ) -> Result<Self> {
        let connect_timeout = Duration::from_secs(config.connect_timeout_secs);
        let started = Instant::now();
        tracing::info!(target: "ssh", "ssh connect: start {}@{}:{} timeout={}s", config.username, config.host, config.port, config.connect_timeout_secs);

        // 1. TCP connect + SSH handshake, both bounded by the shared budget.
        let (session, cancel) = connect_session(&config).await?;

        // 2. Host-key verification (async; may wait for frontend confirmation).
        let hk_started = Instant::now();
        verify_host_key(&session, &config, inquirer, &session_id).await?;
        // 用户读指纹/点确认的时间不计入连接预算（协议段才有「应该多快」一说）。
        let deadline = started + connect_timeout + hk_started.elapsed();
        let remaining = || {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::Ssh("connection timeout (budget exhausted)".into()))
            } else {
                Ok(left)
            }
        };

        // 3. Authentication (blocking). Keyboard-interactive bridges the
        //    prompt to the frontend; its sender is stored on the instance.
        //    注意：kbd 的「等用户输口令」也在这段预算内 —— 要长时间等用户
        //    就调大 connect_timeout_secs（默认 30 s）。
        let (kbd_tx, kbd_rx) = super::auth::kbd_channel();
        let session_for_auth = session.clone();
        let auth_config = config.clone();
        let prompter = super::auth::SshPrompter {
            backend: kb_backend,
            session_id: session_id.clone(),
            rx: kbd_rx,
        };
        run_bounded(&cancel, remaining()?, "auth", move || -> Result<()> {
            match &auth_config.auth {
                super::config::SshAuth::KeyboardInteractive => {
                    let mut p = prompter;
                    session_for_auth
                        .userauth_keyboard_interactive(&auth_config.username, &mut p)
                        .map_err(|e| Error::Ssh(format!("keyboard-interactive auth: {e}")))
                }
                _ => super::auth::authenticate(&session_for_auth, &auth_config),
            }
        })
        .await?;

        // 4. Open PTY shell (blocking).
        let session_for_pty = session.clone();
        let pty_term = config.term.clone();
        let forwarding_options = config.options.clone();
        let channel = run_bounded(&cancel, remaining()?, "pty", move || {
            super::auth::open_pty_shell(
                &session_for_pty,
                cols,
                rows,
                &pty_term,
                &forwarding_options,
            )
        })
        .await?;

        // 5. Interactive loop config + background reader thread.
        session.set_timeout(INTERACTIVE_TIMEOUT_MS);
        // R-21：交互循环必须非阻塞。libssh2 阻塞读会等读缓冲填满或撞上
        // INTERACTIVE_TIMEOUT_MS，几十字节的回显每次都要等满 ~2 s
        // （2026-09-23 实测命令往返 2023/1934/2000 ms，同链路 RTT≈1 ms）。
        // 读线程与 write_channel 各自处理 WouldBlock/TimedOut；没有自带
        // EAGAIN 重试的一次性调用（SFTP / resize / close）用 BlockingGuard
        // 临时切回阻塞，行为与修复前一致。
        session.set_blocking(false);
        drop(cancel); // 仅 drop 克隆句柄不影响连接；只有 shutdown 会掐断
        let alive = Arc::new(AtomicBool::new(true));
        let exit_code = Arc::new(std::sync::Mutex::new(None));
        let connected_at = Arc::new(std::sync::Mutex::new(Instant::now()));
        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);

        let session_arc = Arc::new(Mutex::new(session));
        let channel_arc = Arc::new(Mutex::new(channel));
        let read_channel = channel_arc.lock().await.clone();
        let read_active = Arc::new(AtomicBool::new(true));
        let bytes_written = Arc::new(AtomicUsize::new(0));
        let blocking_mode_gate = Arc::new(std::sync::Mutex::new(()));
        let read_task = super::reconnect::spawn_read_task(
            read_channel,
            tx,
            alive.clone(),
            read_active.clone(),
            exit_code.clone(),
            bytes_written.clone(),
            super::reconnect::ReaderProbe {
                watchdog_secs: config.link_watchdog_secs,
                keepalive_secs: config.options.keepalive_secs,
                session: session_arc.lock().await.clone(),
                blocking_mode_gate: blocking_mode_gate.clone(),
            },
        );

        let kbd_store = if matches!(config.auth, super::config::SshAuth::KeyboardInteractive) {
            Some(kbd_tx)
        } else {
            None
        };

        Ok(Self {
            session: session_arc,
            blocking_mode_gate,
            channel: channel_arc,
            output_rx: Mutex::new(rx),
            alive,
            exit_code,
            keyboard_interactive_tx: Arc::new(std::sync::Mutex::new(kbd_store)),
            config,
            cols,
            rows,
            connected_at,
            read_task: Mutex::new(Some(read_task)),
            read_active: Mutex::new(Some(read_active)),
            bytes_written,
        })
    }

    /// Open an SFTP subsystem channel on this SSH connection.
    ///
    /// Shares the TCP connection but uses a separate channel from the
    /// terminal PTY. The returned `LibSshSftp` implements `SftpTransport`.
    pub async fn open_sftp_channel(&self) -> Result<Arc<dyn crate::sftp::SftpTransport>> {
        let session = self.session.lock().await.clone();
        let session_for_transport = session.clone();
        let gate = self.blocking_mode_gate.clone();
        let gate_for_init = gate.clone();
        let sftp = tokio::task::spawn_blocking(move || {
            let _gate = gate_for_init
                .lock()
                .map_err(|_| "SSH session blocking-mode lock poisoned".to_string())?;
            // R-21 后交互会话是非阻塞的；sftp_init 没有自带 EAGAIN 重试。
            let _blocking = BlockingGuard::new(&session);
            session.sftp().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| Error::Ssh(format!("open sftp task panicked: {e}")))?
        .map_err(|e| Error::Ssh(format!("open sftp: {e}")))?;
        Ok(Arc::new(
            super::sftp::LibSshSftp::new(session_for_transport, sftp, self.alive.clone(), gate)
                .with_creation_modes(
                    self.config.options.sftp_file_mode,
                    self.config.options.sftp_dir_mode,
                ),
        ))
    }
}

/// RAII guard：把共享 session 临时切到阻塞模式，离开作用域恢复原状（R-21）。
///
/// 交互循环以非阻塞跑（否则几十字节的回显要等 8 KiB 读缓冲填满或撞上
/// 2 s 会话超时）；而一次性 SFTP/PTY 调用没有自己的 EAGAIN 重试，直接跑
/// 会立刻失败。ssh2 crate 自身的 SFTP Drop 路径用的就是同一模式。
/// 读线程遵守同一模式锁，避免在临时阻塞窗口抢入空读并等待 2 s。
pub(crate) struct BlockingGuard<'a> {
    session: &'a ssh2::Session,
    was_blocking: bool,
}

impl<'a> BlockingGuard<'a> {
    pub(crate) fn new(session: &'a ssh2::Session) -> Self {
        let was_blocking = session.is_blocking();
        session.set_blocking(true);
        Self {
            session,
            was_blocking,
        }
    }
}

impl Drop for BlockingGuard<'_> {
    fn drop(&mut self) {
        self.session.set_blocking(self.was_blocking);
    }
}

pub(crate) use super::transport::{connect_session, run_bounded};

/// Verify the server host key against known_hosts after the handshake.
///
/// - Known + matching key → accepted.
/// - Unknown host → ask the user via [`HostKeyInquirer`] (60 s timeout);
///   accept learns the key, reject aborts.
/// - Key mismatch → reject (possible MITM).
async fn verify_host_key(
    session: &ssh2::Session,
    config: &SshConfig,
    inquirer: &dyn HostKeyInquirer,
    session_id: &str,
) -> Result<()> {
    let (key, key_type) = session
        .host_key()
        .ok_or_else(|| Error::Ssh("server did not present a host key".into()))?;

    let kh_path = crate::known_hosts::default_known_hosts_path();
    let vs: Result<KnownHostStatus> =
        crate::known_hosts::verify_status(&config.host, config.port, key)
            .map_err(|e| Error::Ssh(e.to_string()));
    tracing::info!(
        target: "ssh",
        "host-key-debug: host={}:{} key_type={:?} kh_path={} file_exists={} verify={:?}",
        config.host, config.port, key_type, kh_path.display(), kh_path.exists(), vs
    );
    match vs {
        Ok(KnownHostStatus::Known) => Ok(()),
        Ok(KnownHostStatus::Unknown) => {
            let key_type_name = host_key_type_name(key_type).to_string();
            let fingerprint = sha256_fingerprint(session.host_key_hash(ssh2::HashType::Sha256));
            match inquirer
                .confirm(
                    session_id,
                    &config.host,
                    config.port,
                    &key_type_name,
                    &fingerprint,
                    key,
                )
                .await
            {
                Ok(true) => {
                    let key_format = KnownHostKeyFormat::from(key_type);
                    if matches!(key_format, KnownHostKeyFormat::Unknown) {
                        return Err(Error::Ssh("unknown host key algorithm".into()));
                    }
                    crate::known_hosts::learn(&config.host, config.port, key, key_format)
                        .map_err(|e| Error::Ssh(e.to_string()))?;
                    Ok(())
                }
                Ok(false) => Err(Error::Ssh(format!(
                    "host key for {}:{} rejected by user",
                    config.host, config.port
                ))),
                Err(e) => Err(Error::Ssh(format!(
                    "host key confirmation failed for {}:{}: {e}",
                    config.host, config.port
                ))),
            }
        }
        Ok(KnownHostStatus::KeyChanged) => Err(Error::Ssh(format!(
            "host key has changed for {}:{} — possible MITM attack",
            config.host, config.port
        ))),
        Err(e) => Err(e),
    }
}

/// Write all bytes to the channel, retrying on WouldBlock/timeout.
/// Runs on a worker thread.
///
/// Bounded by [`WRITE_BUDGET_SECS`]: a stalled/dead socket (no window update,
/// peer gone) fails with an error after the budget instead of retrying forever,
/// which would leak the fire-and-forget write task.
pub fn write_channel(ch: &super::raw_channel::Channel, data: &[u8]) -> Result<()> {
    let mut ch = ch.clone();
    let mut written = 0usize;
    let deadline = Instant::now() + Duration::from_secs(WRITE_BUDGET_SECS);
    while written < data.len() {
        match ch.write(&data[written..]) {
            Ok(0) => return Err(Error::Ssh("write: channel closed".into())),
            Ok(n) => written += n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err(Error::Ssh(format!(
                        "write: no progress after {WRITE_BUDGET_SECS}s ({written}/{} bytes)",
                        data.len()
                    )));
                }
                std::thread::sleep(Duration::from_millis(READ_POLL_MS));
            }
            Err(e) => return Err(Error::Ssh(format!("write: {e}"))),
        }
    }
    Ok(())
}

#[cfg(test)]
mod blocking_guard_tests {
    use super::BlockingGuard;

    /// R-21：guard 必须恢复**进入前**的模式 —— 恢复错方向（比如恒恢复 true）
    /// 会把 SFTP 的一次性调用留在非阻塞上直接失败，反过来则把回显延迟带回 2 s。
    #[test]
    fn blocking_guard_restores_the_previous_mode() {
        let session = ssh2::Session::new().expect("session");

        session.set_blocking(false);
        {
            let _g = BlockingGuard::new(&session);
            assert!(session.is_blocking(), "guard 存续期间必须是阻塞模式");
        }
        assert!(!session.is_blocking(), "离开作用域后必须回到非阻塞");

        session.set_blocking(true);
        {
            let _g = BlockingGuard::new(&session);
            assert!(session.is_blocking());
        }
        assert!(session.is_blocking(), "原本阻塞的会话保持阻塞");
    }
}
