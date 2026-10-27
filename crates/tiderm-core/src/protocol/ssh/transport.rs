//! Bounded TCP, proxy and SSH handshake shared by primary and auxiliary sessions.
use super::config::SshConfig;
use crate::error::{Error, Result};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 在预算内跑一段 blocking 工作；超时或 panic 时对 `cancel` 强制 shutdown，
/// 让被放弃的线程拿到 socket 错误后退出（R-08：外层 timeout 只是「放弃等待」，
/// 不收拾底层资源 —— 必须显式掐断 socket 才能收回线程与句柄）。
pub(crate) async fn run_bounded<T, F>(
    cancel: &TcpStream,
    budget: Duration,
    phase: &str,
    f: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    match tokio::time::timeout(budget, tokio::task::spawn_blocking(f)).await {
        Err(_) => {
            let _ = cancel.shutdown(Shutdown::Both);
            Err(Error::Ssh(format!("connection timeout ({phase})")))
        }
        Ok(Err(p)) => {
            let _ = cancel.shutdown(Shutdown::Both);
            Err(Error::Ssh(format!("{phase} task panicked: {p}")))
        }
        Ok(Ok(r)) => r,
    }
}

/// 有界建连：TCP + SSH 握手共享同一个 deadline（`timeout_secs`）。
///
/// 返回 `(session, cancel)`：`cancel` 是同一条 TCP 连接的克隆句柄，
/// `shutdown` 它可以立刻掐断仍卡在 blocking 调用里的线程 —— 这是
/// 「超时是到点收摊，不只是到点返回」的确定性取消路径。
pub(crate) async fn connect_session(config: &SshConfig) -> Result<(ssh2::Session, TcpStream)> {
    let (host, port, timeout_secs) = (
        config.host.clone(),
        config.port,
        config.connect_timeout_secs.max(1),
    );
    crate::session_options::SessionOptions {
        ssh: config.options.clone(),
        ..Default::default()
    }
    .validate()
    .map_err(Error::Ssh)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    tracing::info!(target: "ssh", "connect_session: {host}:{port} timeout={timeout_secs}s");

    let h = host.clone();
    let options = config.options.clone();
    let tcp = tokio::task::spawn_blocking(move || {
        let proxy = &options.proxy;
        if proxy.kind == 0 {
            return tcp_connect_bounded(&h, port, deadline);
        }
        let mut tcp = tcp_connect_bounded(&proxy.host, proxy.port, deadline)?;
        super::proxy::negotiate(&mut tcp, proxy, &h, port, deadline)?;
        Ok(tcp)
    })
    .await
    .map_err(|e| Error::Ssh(format!("tcp task panicked: {e}")))??;
    let cancel = tcp
        .try_clone()
        .map_err(|e| Error::Ssh(format!("clone socket handle: {e}")))?;

    let timeout_ms = (timeout_secs * 1000).min(u32::MAX as u64) as u32;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        let _ = cancel.shutdown(Shutdown::Both);
        return Err(Error::Ssh("connection timeout (handshake)".into()));
    }
    let addr = format!("{host}:{port}");
    let options = config.options.clone();
    let session = run_bounded(&cancel, remaining, "handshake", move || {
        handshake_session(tcp, timeout_ms, &addr, &options)
    })
    .await?;
    Ok((session, cancel))
}

/// 解析并连接首个可达地址；每个地址的超时 = 剩余预算（R-08：
/// `TcpStream::connect` 本身无超时，SYN 黑洞会让 blocking 线程无限挂）。
fn tcp_connect_bounded(host: &str, port: u16, deadline: Instant) -> Result<TcpStream> {
    let addr = format!("{host}:{port}");
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| Error::Ssh(format!("resolve {addr}: {e}")))?
        .collect();
    let mut last = None;
    for a in addrs {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&a, remaining) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(Error::Ssh(format!("connect to {a}: {e}"))),
        }
    }
    Err(last.unwrap_or_else(|| Error::Ssh(format!("connect to {addr}: timed out"))))
}

/// 阻塞式 SSH 握手（工作线程）。libssh2 的会话超时对静默主机的 banner
/// 等待不生效（修复前实测 4 次超时建连后连接全部存活），真正的兜底是
/// 外层 [`run_bounded`] 的 cancel shutdown。
fn handshake_session(
    tcp: TcpStream,
    timeout_ms: u32,
    addr: &str,
    options: &crate::session_options::SshOptions,
) -> Result<ssh2::Session> {
    // Avoid Nagle / delayed-ACK stalls on bulk transfers.
    let _ = tcp.set_nodelay(true);
    let mut session = ssh2::Session::new().map_err(|e| Error::Ssh(format!("init session: {e}")))?;
    super::negotiation::configure(&session, options)?;
    session.set_tcp_stream(tcp);
    session.set_blocking(true);
    session.set_timeout(timeout_ms);
    tracing::info!(target: "ssh", "connect: tcp established {addr}, handshaking");
    session
        .handshake()
        .map_err(|e| Error::Ssh(format!("handshake with {addr}: {e}")))?;
    Ok(session)
}
