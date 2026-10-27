//! SSH 链路集成测试 —— 两类：
//!
//! 1. **本地故障注入**（无需外部服务器）：起一个「接受 TCP 但永不回包」的哑 listener，
//!    复现被防火墙 tarpit / sshd 卡死的主机形态，用于验证连接超时是否真的有界、
//!    以及失败路径是否泄漏 blocking 线程。默认 `#[ignore]`（会故意留驻 parked 线程）。
//! 2. **真实 SSH 旅程**：需 `TEST_SSH_HOST/USER/PASS/KNOWN_HOSTS`，默认 `#[ignore]`。
//!
//! 运行：
//!   cargo test protocol::ssh::env_tests -- --ignored --nocapture
//! 真实机（凭据只走环境变量，不写进仓库）：
//!   TEST_SSH_KNOWN_HOSTS 必须指向系统临时目录内的专用 fixture；禁止使用 ~/.ssh。
//!   cargo test env_tests::real_ssh -- --ignored --nocapture

use super::config::{SshAuth, SshConfig};
use super::connection::SshInstance;
use crate::auth::{HostKeyInquirer, KbSyncBackend};
use crate::protocol::Protocol;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 自动接受主机密钥的桩；真实 SSH 用例只写临时 known_hosts fixture。
struct AutoAccept;

#[async_trait::async_trait]
impl HostKeyInquirer for AutoAccept {
    async fn confirm(
        &self,
        _session_id: &str,
        _host: &str,
        _port: u16,
        _key_type: &str,
        _fingerprint: &str,
        _key: &[u8],
    ) -> Result<bool, String> {
        Ok(true)
    }
}

struct NoKb;
impl KbSyncBackend for NoKb {
    fn emit(
        &self,
        _session_id: &str,
        _name: &str,
        _instruction: &str,
        _fields: &[crate::event::ChallengeField],
    ) -> std::result::Result<(), String> {
        Err("测试内不应触发 keyboard-interactive".into())
    }
}

fn cfg(host: &str, port: u16, timeout_secs: u64) -> SshConfig {
    SshConfig {
        host: host.into(),
        port,
        username: "probe".into(),
        auth: SshAuth::Password {
            password: "probe".into(),
        },
        connect_timeout_secs: timeout_secs,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 0,
        link_watchdog_secs: 0,
        // 本地故障注入测例默认关看门狗：哑 listener 永远不会回字节，
        // 而这些用例要量的正是"没有看门狗时的行为"。
        options: Default::default(),
        term: "xterm-256color".into(),
    }
}

/// 只 accept、永不写字节、永不关闭 —— 连接方会卡在等 SSH 识别串。
/// 同时充当「对端视角」：统计仍存活的连接数，用来判定客户端是否真的放弃了底层 socket。
fn silent_hole() -> (u16, Arc<std::sync::Mutex<Vec<TcpStream>>>) {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    let held: Arc<std::sync::Mutex<Vec<TcpStream>>> = Default::default();
    let sink = held.clone();
    std::thread::spawn(move || {
        for incoming in l.incoming() {
            match incoming {
                Ok(s) => {
                    let _ = s.set_nonblocking(true);
                    sink.lock().expect("held").push(s);
                }
                Err(_) => break,
            }
        }
    });
    (port, held)
}

/// 仍打开的连接数：**先把已缓冲数据排空**再判 —— 客户端一进握手就会发出
/// banner 字节，若只读一次，会把「已经 FIN 但缓冲里还有 banner」的连接
/// 永远误判成存活（2026-09-24 探针证实：clone.shutdown 后服务端首读仍得
/// Ok(1)=banner）。排空后读到 0 = 对端已关；WouldBlock = 仍开放。
fn live_connections(held: &std::sync::Mutex<Vec<TcpStream>>) -> usize {
    let mut v = held.lock().expect("held");
    let mut live = 0usize;
    for s in v.iter_mut() {
        let mut b = [0u8; 64];
        loop {
            match s.read(&mut b) {
                Ok(0) => break, // EOF：对端已关闭
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    live += 1;
                    break;
                }
                Err(_) => break,
            }
        }
    }
    live
}

/// CON-11：连接被 tarpit 的主机时，用户可见的失败必须被 `connect_timeout_secs` 约束住。
#[tokio::test]
#[ignore = "本地故障注入，会故意留驻 parked 线程"]
async fn connect_timeout_is_bounded_for_silent_host() {
    let (port, _held) = silent_hole();
    let budget = 3u64;
    let t0 = Instant::now();
    let r = SshInstance::connect(
        cfg("127.0.0.1", port, budget),
        80,
        24,
        "silenthole".into(),
        &AutoAccept,
        Arc::new(NoKb),
    )
    .await;
    let el = t0.elapsed();
    assert!(r.is_err(), "哑主机不该连接成功");
    println!(
        "[CON-11] timeout_secs={budget} elapsed_ms={} err={:?}",
        el.as_millis(),
        r.err().unwrap()
    );
    assert!(
        el < Duration::from_secs(budget + 2),
        "用户可见失败耗时 {} ms 远超配置的 {} s —— 超时未生效",
        el.as_millis(),
        budget
    );
}

/// R-08：超时建连后 socket 与线程必须被回收。
///
/// 2026-09-24 修正认知（原判「线程与 socket 永久滞留」部分是预言机误报）：
/// - 客户端进入握手即发出 banner 字节，旧预言机只读一次，把「已 FIN 但
///   缓冲有 banner」判成存活 —— 修复前测出的「4 条全泄漏」混入了假阳性；
/// - 探针实证 libssh2 阻塞握手的 `set_timeout` **自超时是生效的**（10 s 配置
///   精确 10.008 s 返回），线程并非永久滞留；
/// - 真正无界的是 **TCP 建连段**（`TcpStream::connect` 无超时，SYN 黑洞会
///   无限挂）与**认证/开 PTY 段**（= R-20）。两者已分别由
///   `tcp_connect_bounded` 与 `run_bounded` 预算修复，超时路径还会对 socket
///   强制 shutdown，让服务端立刻看到 FIN。
#[tokio::test]
#[ignore = "本地故障注入，会故意留驻 parked 线程"]
async fn timed_out_connect_releases_socket_and_thread() {
    let (port, held) = silent_hole();
    for i in 0..4 {
        let r = SshInstance::connect(
            cfg("127.0.0.1", port, 1),
            80,
            24,
            format!("leak{i}"),
            &AutoAccept,
            Arc::new(NoKb),
        )
        .await;
        assert!(r.is_err());
    }
    // 外层早已返回 Err；再等 3 s，若真取消则底层 TcpStream 应随闭包 drop 而关闭。
    tokio::time::sleep(Duration::from_secs(3)).await;
    let live = live_connections(&held);
    println!(
        "[R-08] 4 次超时建连后仍存活的客户端连接 = {live}（期望 0，等于 4 即线程与 socket 全泄漏）"
    );
    assert_eq!(
        live, 0,
        "超时后仍持有 {live} 条连接：connect_handshake 的 blocking 线程未被取消，\
         每个不可达主机永久占住 1 个 tokio blocking 线程 + 1 个 socket（池上限 512 → 全应用 I/O 停摆）"
    );
}

// ── 真机疲劳与吞吐完整性（SOAK / R-03 / R-04 / R-11 的经验判据）─────────────

fn sample_self_raw() -> u64 {
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(std::process::id());
    sys.refresh_process(pid);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

/// 仪器自检：`sysinfo::Process::memory()` 的单位不靠猜 —— 分配一块已知大小的内存，
/// 看计数器走了多少格。仓库 `sftp.rs` 里那个叫 `current_memory_kb()` 的 helper 用的
/// 就是同一 API，若单位实为字节则其名字会误导读数。
///
/// 返回 `(单位, 实测步长)`；步长落在两个候选区间之外时判 `indeterminate`，调用方须据此
/// 放弃内存判据而不是拿一个含义不明的数字下结论。
fn calibrate_memory_unit() -> (&'static str, u64) {
    const BLOCK: u64 = 64 * 1024 * 1024; // 64 MiB
    let before = sample_self_raw();
    let mut block = vec![0u8; BLOCK as usize];
    for b in block.iter_mut() {
        *b = 1; // 逐页触写，确保真的进工作集
    }
    std::hint::black_box(&block);
    let after = sample_self_raw();
    std::hint::black_box(&block);
    drop(block);

    let step = after.saturating_sub(before);
    let unit = if step > BLOCK / 2 {
        "bytes"
    } else if step > BLOCK / 2 / 1024 {
        "KiB"
    } else {
        "indeterminate"
    };
    (unit, step)
}

/// 服务端视角的连接数预言机：在活会话里跑 `ss` 数已建立 TCP 连接。
/// 注意命令回显里含的是 `%s` 而非数字，所以 `TTCONN <数字>` 只可能来自真实输出，
/// 不会像裸匹配标记那样被终端回显骗过（见测试文档 §7 教训 3）。
async fn server_estab_tcp(s: &SshInstance) -> Option<usize> {
    s.write(b"printf 'TTCONN %s\\n' \"$(ss -H -tn state established 2>/dev/null | wc -l)\"\r")
        .await
        .ok()?;
    let mut acc = String::new();
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
            acc.push_str(&String::from_utf8_lossy(&b));
            if let Some(n) = parse_ttconn(&acc) {
                return Some(n);
            }
        }
    }
    None
}

fn parse_ttconn(acc: &str) -> Option<usize> {
    let (_, rest) = acc.rsplit_once("TTCONN ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn real_cfg() -> SshConfig {
    let (host, port, user, pass) = env_all();
    cfg_on(host, port, user, pass, 15)
}

fn cfg_on(host: String, port: u16, user: String, pass: String, timeout_secs: u64) -> SshConfig {
    SshConfig {
        host,
        port,
        username: user,
        auth: SshAuth::Password { password: pass },
        connect_timeout_secs: timeout_secs,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 0,
        link_watchdog_secs: 0,
        options: Default::default(),
        term: "xterm-256color".into(),
    }
}

/// 走本机弱网代理建连（host 固定为 127.0.0.1，凭据仍取环境变量的真实账号）。
async fn open_via_proxy(tag: &str, port: u16, timeout_secs: u64) -> Option<SshInstance> {
    let (_, _, user, pass) = env_all();
    let c = cfg_on("127.0.0.1".into(), port, user, pass, timeout_secs);
    SshInstance::connect(c, 80, 24, tag.into(), &AutoAccept, Arc::new(NoKb))
        .await
        .ok()
}

async fn open_once(tag: &str) -> Option<SshInstance> {
    let c = real_cfg();
    match SshInstance::connect(c, 80, 24, tag.into(), &AutoAccept, Arc::new(NoKb)).await {
        Ok(s) => Some(s),
        Err(e) => {
            // 失败原因必须可见：SOAK-02 的 30/30 断言红时，先分清是哪一段挂了。
            eprintln!("[open_once:{tag}] connect failed: {e:?}");
            None
        }
    }
}

/// SOAK-02 / R-03 / R-04：反复「建连 → 退出 → disconnect」不得累积线程与内存。
/// 同时给出真机重连成本分布（供 §4 重连退避参数取值）。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS；跑满数分钟"]
async fn real_ssh_session_churn_does_not_accumulate_resources() {
    const N: usize = 30;
    // 缺少任一前置条件时，显式运行必须失败。
    env_all();
    // 先跑 3 轮把 blocking 池与 TLS/libssh2 惰性初始化摊平，再取基线。
    for i in 0..3 {
        let s = open_once(&format!("warm{i}"))
            .await
            .expect("热身连接失败，不能继续采集基线");
        s.write("exit\r".as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        s.disconnect().await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 连接数口径：基线与末值各用一条独立「测量会话」读 ss，两边都含这 1 条，可直接比差值。
    let mut probe = Some(open_once("probe-base").await.expect("基线探针连接失败"));
    let c_base = match probe.as_ref() {
        Some(s) => server_estab_tcp(s).await,
        None => None,
    }
    .expect("基线服务端连接数探针不可用，需要靶机提供 ss");
    if let Some(s) = probe.take() {
        let _ = s.disconnect().await;
    }
    // 先校准计数器单位（不猜），再取基线；基线在释放校准块之后取，避免残留计入。
    let (unit, step) = calibrate_memory_unit();
    println!("[SOAK-02] 内存计数器单位实测 = {unit}（64 MiB 试块使计数器走 {step} 格）");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let m_base = sample_self_raw();
    let mem_gate = unit != "indeterminate";

    let mut lat = Vec::with_capacity(N);
    let mut opened = 0usize;
    for i in 0..N {
        let t = Instant::now();
        let Some(s) = open_once(&format!("churn{i}")).await else {
            continue;
        };
        opened += 1;
        lat.push(t.elapsed().as_millis() as u64);
        s.write("exit\r".as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        s.disconnect().await.unwrap();
    }
    // 给被放弃的读任务留出退出时间；若仍不回落即为累积。
    tokio::time::sleep(Duration::from_secs(5)).await;
    let mut probe = Some(open_once("probe-after").await.expect("结束探针连接失败"));
    let c_after = match probe.as_ref() {
        Some(s) => server_estab_tcp(s).await,
        None => None,
    }
    .expect("结束服务端连接数探针不可用，需要靶机提供 ss");
    if let Some(s) = probe.take() {
        let _ = s.disconnect().await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let m_after = sample_self_raw();

    lat.sort_unstable();
    let pct = |p: usize| lat[(lat.len() * p / 100).min(lat.len().saturating_sub(1))];
    println!(
        "[SOAK-02] cycles={opened}/{N} 重连耗时 min={}ms p50={}ms p95={}ms max={}ms",
        lat.first().copied().unwrap_or(0),
        if lat.is_empty() { 0 } else { pct(50) },
        if lat.is_empty() { 0 } else { pct(95) },
        lat.last().copied().unwrap_or(0)
    );
    // 单位来自上面的自检，不靠猜；indeterminate 时只报原始值、不下内存结论。
    let mib = |v: u64| match unit {
        "bytes" => v as f64 / 1_048_576.0,
        "KiB" => v as f64 / 1_024.0,
        _ => 0.0,
    };
    println!(
        "[SOAK-02] server_estab_tcp {c_base:?} -> {c_after:?}; self_mem {:.1} -> {:.1} MiB \
         (raw {m_base} -> {m_after}, unit={unit})",
        mib(m_base),
        mib(m_after)
    );
    assert_eq!(
        opened,
        N,
        "{}/{} 次建连失败 —— 稳定性本身不达标",
        N - opened,
        N
    );
    assert!(
        c_after.saturating_sub(c_base) <= 2,
        "{N} 轮「建连→退出→disconnect」后服务端仍见 {} 条多余已建立连接\
         （R-03/R-04/R-08：会话句柄或底层 socket 未回收）",
        c_after.saturating_sub(c_base)
    );
    assert!(mem_gate, "内存计数器单位无法校准，不能判定资源是否回落");
    assert!(
        mib(m_after) - mib(m_base) <= 8.0,
        "{N} 轮会话后自身内存净增 {:.1} MiB —— 需人工分辨缓存还是泄漏（R-03）",
        mib(m_after) - mib(m_base)
    );
}

fn env_all() -> (String, u16, String, String) {
    let host = std::env::var("TEST_SSH_HOST").expect("需要 TEST_SSH_HOST");
    let user = std::env::var("TEST_SSH_USER").expect("需要 TEST_SSH_USER");
    let pass = std::env::var("TEST_SSH_PASS").expect("需要 TEST_SSH_PASS");
    assert!(
        !host.is_empty() && !user.is_empty() && !pass.is_empty(),
        "TEST_SSH_HOST/USER/PASS 不能为空"
    );
    let fixture = std::path::PathBuf::from(
        std::env::var_os("TEST_SSH_KNOWN_HOSTS").expect("需要 TEST_SSH_KNOWN_HOSTS"),
    );
    assert!(
        fixture.is_absolute() && fixture.starts_with(std::env::temp_dir()),
        "TEST_SSH_KNOWN_HOSTS 必须在系统临时目录内，不能指向用户 known_hosts"
    );
    std::fs::create_dir_all(fixture.parent().expect("fixture parent"))
        .expect("无法创建 known_hosts fixture 目录");
    assert!(
        fixture
            .parent()
            .unwrap()
            .canonicalize()
            .expect("fixture parent canonicalize")
            .starts_with(
                std::env::temp_dir()
                    .canonicalize()
                    .expect("temp canonicalize")
            ),
        "known_hosts fixture 目录不能通过符号链接跳出系统临时目录"
    );
    assert!(
        !fixture.is_symlink(),
        "known_hosts fixture 文件不能是符号链接"
    );
    let port = std::env::var("TEST_SSH_PORT")
        .map(|p| p.parse::<u16>().expect("TEST_SSH_PORT 必须是有效端口"))
        .unwrap_or(22);
    assert_ne!(port, 0, "TEST_SSH_PORT 不能是零");
    (host, port, user, pass)
}

/// 读到 shell 提示符为止；途中收到的字节留在 `acc` 里，不丢字。
async fn wait_prompt(s: &SshInstance, acc: &mut String) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
            acc.push_str(&String::from_utf8_lossy(&b));
            if acc.contains('$') || acc.contains('#') {
                return;
            }
        }
    }
}

mod terminal;

mod weak_net;
