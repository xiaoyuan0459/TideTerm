//! 会话宿主（Phase C ④ 重写）—— tiderm-core 协议栈版。
//!
//! 五协议真实连接（本地 PTY / SSH / Telnet / Serial / TCP）：
//! - 连接与读循环跑在共享 tokio runtime（`'static` OnceLock）；
//! - 输出经 `BackendEventSink` broadcast 回 UI 线程，[`crate::rust::term`] 的
//!   poll 统一 drain 并按 uid 路由进各标签；
//! - SSH 主机密钥确认走 `EventHostKeyInquirer`（emit + oneshot，60s 超时自动
//!   拒绝——安全铁律：绝不静默接受），keyboard-interactive 走同步 emit +
//!   `Protocol::respond_keyboard_interactive` 应答；
//! - 输入经有界 FIFO 单写任务；resize 异步执行，UI 不阻塞 I/O；
//! - 远程连接完成前输入仍进同一队列，交付后按原顺序回放。
//!
//! 替代 Phase B 的 hyperpanes-core SessionManager 本地 PTY 桥（strangler：
//! 引擎渲染由 tiderm-terminal 提供，传输全部归 tiderm-core）。

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use super::session_modem::ProtoSource;
use tiderm_core::auth::{HostKeyInquirer, KbSyncBackend};
use tiderm_core::event::BackendEvent;
use tiderm_core::protocol::pty_impl::PtyInstance;
use tiderm_core::protocol::serial::SerialInstance;
use tiderm_core::protocol::ssh::SshInstance;
use tiderm_core::protocol::{Protocol, TcpInstance, TelnetInstance};
use tiderm_core::read_loop::spawn_read_loop;

use super::events::{discard_delivery, host_key, push_delivery, sink, SinkKbBackend};
use crate::rust::store::session_config::SessionConfig;

/// 全部会话共享一个多线程 runtime（c1 定案：不逐宿主建 runtime）。
pub fn shared_runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

// ════════════════════ SessionHost ════════════════════

pub struct SessionHost {
    pub uid: String,
    /// 已连接协议（本地 PTY 立即有；远程在 connect 任务交付后有）。
    proto: RefCell<Option<Arc<dyn Protocol>>>,
    /// Keep a private startup file alive until this host is retired.
    shell_init: RefCell<Option<tempfile::NamedTempFile>>,
    pub(super) input: super::session_input::SessionInput,
    input_error: Cell<bool>,
    codec: super::session_codec::SessionCodec,
    /// Prevent a connect task from delivering a transport after its tab closes.
    active: Arc<AtomicBool>,
    pub(super) modem: tiderm_core::modem::Router,
}

/// Automatic reconnect may only reuse a key already trusted by known_hosts.
struct RejectUnknownHostKey;

#[async_trait]
impl HostKeyInquirer for RejectUnknownHostKey {
    async fn confirm(
        &self,
        _session_id: &str,
        _host: &str,
        _port: u16,
        _key_type: &str,
        _fingerprint: &str,
        _key: &[u8],
    ) -> Result<bool, String> {
        Ok(false)
    }
}

impl SessionHost {
    /// 按配置建会话：本地 PTY 同步起；远程 spawn connect（失败经 Error 事件）。
    pub fn start(uid: String, cfg: &SessionConfig, cols: u16, rows: u16) -> Self {
        Self::start_with_mode(uid, cfg, cols, rows, false)
    }

    pub fn start_auto(uid: String, cfg: &SessionConfig, cols: u16, rows: u16) -> Self {
        Self::start_with_mode(uid, cfg, cols, rows, true)
    }

    fn start_with_mode(
        uid: String,
        cfg: &SessionConfig,
        cols: u16,
        rows: u16,
        known_only: bool,
    ) -> Self {
        super::events::prepare_receiver();
        let host = SessionHost {
            uid: uid.clone(),
            proto: RefCell::new(None),
            shell_init: RefCell::new(None),
            modem: Default::default(),
            input: Default::default(),
            input_error: Cell::new(false),
            codec: super::session_codec::SessionCodec::new(
                if cfg.tcp.as_ref().is_some_and(|c| c.data_mode == 1) {
                    "UTF-8"
                } else {
                    &cfg.meta.charset
                },
            ),
            active: Arc::new(AtomicBool::new(true)),
        };
        use crate::rust::store::session_config as sc;
        match cfg.kind {
            sc::SessionKind::Local => {
                let blank = sc::LocalConfig {
                    shell: String::new(),
                    shell_path: String::new(),
                    args: String::new(),
                    working_directory: String::new(),
                };
                let l = cfg.local.clone().unwrap_or(blank);
                let cwd = l.working_directory.clone();
                let pty_term = if cfg.meta.terminal_emulation.is_empty() {
                    "xterm-256color".to_string()
                } else {
                    cfg.meta.terminal_emulation.clone()
                };
                // 本地 PTY 同步 spawn（core API 同步）；read_loop 上 runtime。
                let _guard = shared_runtime().enter();
                match super::local_shell::launch_options(&l).and_then(|launch| {
                    let pty = PtyInstance::spawn_with_options(
                        cols,
                        rows,
                        &launch.program,
                        &launch.args,
                        if cwd.is_empty() {
                            None
                        } else {
                            Some(cwd.as_str())
                        },
                        &pty_term,
                    )?;
                    *host.shell_init.borrow_mut() = launch.init;
                    Ok(pty)
                }) {
                    Ok(pty) => {
                        let proto: Arc<dyn Protocol> = Arc::new(pty);
                        spawn_read_loop(
                            uid,
                            Arc::new(ProtoSource(proto.clone(), host.modem.clone())),
                            sink().clone(),
                        );
                        host.input.attach(host.uid.clone(), proto.clone());
                        *host.proto.borrow_mut() = Some(proto);
                    }
                    Err(e) => {
                        sink().emit(BackendEvent::Error {
                            id: uid,
                            message: format!("本地会话启动失败：{e}"),
                        });
                    }
                }
            }
            sc::SessionKind::Ssh => match to_proto_ssh(cfg) {
                Ok(c) => {
                    let hk: Arc<dyn HostKeyInquirer> = if known_only {
                        Arc::new(RejectUnknownHostKey)
                    } else {
                        host_key().clone()
                    };
                    let kb: Arc<dyn KbSyncBackend> = Arc::new(SinkKbBackend);
                    let task_uid = uid.clone();
                    spawn_connect(&uid, host.active.clone(), async move {
                        SshInstance::connect(c, cols, rows, task_uid.clone(), hk.as_ref(), kb)
                            .await
                            .map(|s| Arc::new(s) as Arc<dyn Protocol>)
                    });
                }
                Err(e) => emit_error(&uid, &e),
            },
            sc::SessionKind::Telnet => {
                let t = tiderm_core::protocol::telnet::TelnetConfig {
                    host: cfg
                        .telnet
                        .as_ref()
                        .map(|t| t.host.clone())
                        .unwrap_or_default(),
                    port: cfg.telnet.as_ref().map(|t| t.port).unwrap_or(23),
                    terminal_type: cfg
                        .telnet
                        .as_ref()
                        .map(|t| t.terminal_type.clone())
                        .unwrap_or_else(|| "xterm".into()),
                    connect_timeout_secs: 30,
                };
                spawn_connect(&uid, host.active.clone(), async move {
                    TelnetInstance::connect(t, cols, rows)
                        .await
                        .map(|s| Arc::new(s) as Arc<dyn Protocol>)
                });
            }
            sc::SessionKind::Tcp => {
                let t = tiderm_core::protocol::tcp::TcpConfig {
                    host: cfg.tcp.as_ref().map(|t| t.host.clone()).unwrap_or_default(),
                    port: cfg.tcp.as_ref().map(|t| t.port).unwrap_or(23),
                    terminal_type: cfg
                        .tcp
                        .as_ref()
                        .map(|t| t.terminal_type.clone())
                        .unwrap_or_else(|| "xterm".into()),
                    connect_timeout_secs: 30,
                    data_mode: if cfg.tcp.as_ref().map(|t| t.data_mode) == Some(1) {
                        "binary".into()
                    } else {
                        "text".into()
                    },
                };
                spawn_connect(&uid, host.active.clone(), async move {
                    TcpInstance::connect(t, cols, rows)
                        .await
                        .map(|s| Arc::new(s) as Arc<dyn Protocol>)
                });
            }
            sc::SessionKind::Serial => {
                let blank = sc::SerialConfig {
                    path: String::new(),
                    baud_rate: 115200,
                    data_bits: 8,
                    stop_bits: 1,
                    parity: 0,
                    flow_control: 0,
                };
                let s = cfg.serial.clone().unwrap_or(blank);
                let c = tiderm_core::protocol::serial::SerialConfig {
                    path: s.path,
                    baud_rate: s.baud_rate,
                    data_bits: s.data_bits,
                    stop_bits: s.stop_bits,
                    parity: match s.parity {
                        1 => "odd".into(),
                        2 => "even".into(),
                        _ => "none".into(),
                    },
                    flow_control: match s.flow_control {
                        1 => "hardware".into(),
                        2 => "xonxoff".into(),
                        _ => "none".into(),
                    },
                };
                spawn_connect(&uid, host.active.clone(), async move {
                    SerialInstance::connect(c)
                        .await
                        .map(|s| Arc::new(s) as Arc<dyn Protocol>)
                });
            }
        }
        host
    }

    /// Enqueue without blocking UI; the transport has exactly one FIFO writer.
    pub fn write(&self, bytes: &[u8]) {
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        let bytes = match self.codec.encode(bytes) {
            Ok(bytes) => bytes,
            Err(message) => {
                emit_error(&self.uid, &message);
                return;
            }
        };
        self.write_wire(&bytes);
    }

    /// Explicit jobs need an admission result so a full/closed FIFO stops their schedule.
    pub fn queue_text(&self, bytes: &[u8]) -> Result<(), &'static str> {
        if !self.active.load(Ordering::SeqCst) || !self.alive() || self.modem.busy() {
            return Err("会话已关闭或正在传输文件");
        }
        let encoded = self.codec.encode(bytes).map_err(|_| "字符集转换失败")?;
        self.input.write(&encoded)
    }

    /// Hex Sender jobs need the same bounded FIFO admission without encoding.
    pub fn queue_wire(&self, bytes: &[u8]) -> Result<(), &'static str> {
        if !self.active.load(Ordering::SeqCst) || !self.alive() || self.modem.busy() {
            return Err("会话已关闭或正在传输文件");
        }
        self.input.write(bytes)
    }

    /// Raw sender bytes bypass character-set encoding (Hex/file/modem data).
    pub fn write_wire(&self, bytes: &[u8]) {
        if self.modem.busy() {
            return;
        }
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        match self.input.write(bytes) {
            Ok(()) => self.input_error.set(false),
            Err(message) if !self.input_error.replace(true) => emit_error(&self.uid, message),
            Err(_) => {}
        }
    }

    /// Queue grid changes in the same order as keyboard input, including startup.
    pub fn resize(&self, cols: u16, rows: u16) {
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        if let Err(error) = self.input.resize(cols, rows) {
            emit_error(&self.uid, error);
        }
    }

    /// connect 任务交付协议 + 回放 pending 写缓冲（term poll 领取时调）。
    pub fn deliver(&self, p: Arc<dyn Protocol>) {
        // `spawn_read_loop` 内部是裸 `tokio::spawn`，而本方法由 term 泵线程调用 ——
        // 该线程没有 entered runtime 时直接 panic「there is no reactor running」，
        // 于是**任何远程会话在第一次泵到时就把进程打断**。与本地 PTY 分支
        // （`open` 里的 `shared_runtime().enter()`）同构。
        let _guard = shared_runtime().enter();
        if !self.active.load(Ordering::SeqCst) {
            shared_runtime().spawn(async move {
                let _ = p.disconnect().await;
            });
            return;
        }
        spawn_read_loop(
            self.uid.clone(),
            Arc::new(ProtoSource(p.clone(), self.modem.clone())),
            sink().clone(),
        );
        self.input.attach(self.uid.clone(), p.clone());
        *self.proto.borrow_mut() = Some(p);
    }

    pub fn connected(&self) -> bool {
        self.proto.borrow().is_some()
    }

    /// 协议句柄快照（SFTP 开 channel 等跨域能力用）。
    pub fn protocol(&self) -> Option<Arc<dyn Protocol>> {
        self.proto.borrow().clone()
    }

    /// 应答键盘交互挑战（协议已交付时转发；否则 Err 给 UI 提示）。
    pub fn respond_kb(&self, responses: Vec<String>) -> Result<(), String> {
        match &*self.proto.borrow() {
            Some(p) => p
                .respond_keyboard_interactive(responses)
                .map_err(|e| e.to_string()),
            None => Err("会话尚未完成连接，无法应答".into()),
        }
    }

    pub fn alive(&self) -> bool {
        self.proto.borrow().as_ref().is_some_and(|p| p.is_alive())
    }

    /// Close transport promptly, including the local PTY held by its read loop.
    pub fn disconnect(&self) {
        self.modem.cancel();
        self.active.store(false, Ordering::SeqCst);
        discard_delivery(&self.uid);
        self.input.close();
        if let Some(proto) = self.proto.borrow_mut().take() {
            shared_runtime().spawn(async move {
                let _ = proto.disconnect().await;
            });
        }
    }
}

impl Drop for SessionHost {
    fn drop(&mut self) {
        self.disconnect();
    }
}

pub(super) fn emit_error(uid: &str, msg: &str) {
    let _guard = shared_runtime().enter();
    sink().emit(BackendEvent::Error {
        id: uid.to_string(),
        message: msg.to_string(),
    });
}

/// native 表单 SSH 配置 → protocol 侧 `SshConfig`（backend_support 同式薄映射；
/// OneKey 需账户面板解析后才可连接——与 Slint 壳同语义）。
fn to_proto_ssh(c: &SessionConfig) -> Result<tiderm_core::protocol::ssh::SshConfig, String> {
    use crate::rust::store::session_config::SshAuth;
    let s = c.ssh.as_ref().ok_or("缺少 SSH 配置")?;
    let mut username = s.username.clone();
    let auth = match s.auth {
        SshAuth::Password => tiderm_core::protocol::ssh::SshAuth::Password {
            password: s.password.clone(),
        },
        SshAuth::Key => tiderm_core::protocol::ssh::SshAuth::PublicKey {
            private_key_path: s.private_key_path.clone(),
            passphrase: if s.passphrase.is_empty() {
                None
            } else {
                Some(s.passphrase.clone())
            },
        },
        SshAuth::KeyboardInteractive => tiderm_core::protocol::ssh::SshAuth::KeyboardInteractive,
        SshAuth::Agent => tiderm_core::protocol::ssh::SshAuth::Agent,
        SshAuth::OneKey => {
            let (account_user, auth) = super::onekey::resolve_auth(&s.account_id)?;
            username = account_user;
            auth
        }
    };
    Ok(tiderm_core::protocol::ssh::SshConfig {
        options: c.meta.options.ssh.clone(),
        host: s.host.clone(),
        port: s.port,
        username,
        auth,
        connect_timeout_secs: if s.connect_timeout_secs == 0 {
            15
        } else {
            s.connect_timeout_secs as u64
        },
        reconnect_enabled: s.reconnect_enabled,
        max_reconnect_attempts: s.max_reconnect_attempts,
        reconnect_interval_secs: s.reconnect_interval_secs.max(1) as u64,
        // 静默链路看门狗（R-16）默认值，与 core `SshConfig::link_watchdog_secs` 一致。
        // 壳层表单暂不暴露该旋钮 —— 记录为待办：Qt 侧加一项后才允许用户自行关掉。
        link_watchdog_secs: c.meta.options.ssh.watchdog_secs as u64,
        // PTY 请求的终端类型（会话表单「终端:(T)」）。
        term: if c.meta.terminal_emulation.is_empty() {
            "xterm-256color".into()
        } else {
            c.meta.terminal_emulation.clone()
        },
    })
}

/// spawn 远程连接任务：成功 → 交付队列（term poll 领取时交付 + 起 read_loop）；
/// 失败 → Error 事件。不在握手期间持任何 UI 侧锁——host key 确认需要 UI 线程
/// 在握手期间应答（backend_model 模式）。
fn spawn_connect(
    uid: &str,
    active: Arc<AtomicBool>,
    fut: impl std::future::Future<Output = Result<Arc<dyn Protocol>, tiderm_core::error::Error>>
        + Send
        + 'static,
) {
    let uid = uid.to_string();
    shared_runtime().spawn(async move {
        match fut.await {
            Ok(proto) => {
                if let Err(proto) = push_delivery(uid, &active, proto) {
                    let _ = proto.disconnect().await;
                }
            }
            Err(e) => {
                sink().emit(BackendEvent::Error {
                    id: uid,
                    message: e.to_string(),
                });
            }
        }
    });
}
