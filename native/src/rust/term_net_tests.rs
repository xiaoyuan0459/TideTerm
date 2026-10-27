//! 壳层（native）连接生命周期探针：不开 GUI、不碰真服务器、不需要任何凭据。
//!
//! 补的是 §0.5 留下的缺口：R-01/R-02/R-03 的**入口在壳层**（标签页 / SFTP 面板），
//! core 侧 `env_tests` 只驱动 `SshInstance` 本身，够不到；此前只能标"未测"。
//! 手法：用 `SessionKind::Tcp` 开真标签打到本机 `TcpListener`，
//! 预言机取**对端仍存活的连接数**（非阻塞 read 得 `WouldBlock` = 活着，得 0 字节 = 已关）——
//! 与 core 侧 R-08 探针同一口径，零依赖、CI 可复跑、直指"句柄未释放"。
//! 不用线程数：本机 sysinfo 构建里没有 `Process::nb_threads()`。
//!
//! **R-17（本轮已修，守卫留在常规门禁里）**：
//! `SessionHost::deliver()` 直接调 `spawn_read_loop()`，而后者用裸 `tokio::spawn`；
//! 泵线程（C++ GUI 线程，`cpp/main.cpp:237/249/256/261`）没有 `shared_runtime().enter()`，
//! 于是**远程会话一旦连上、下一次泵就 panic**。本地 PTY 不出事，因为它在
//! `session.rs:96` 显式取了 guard —— 同一个仓库里两种写法，说明这是漏改不是设计。
//! 修法即在 `deliver()` 开头补 guard；本文件所有测例都**故意不再自行 enter**，
//! 与真实泵线程同处境，这样 R-17 若复发，四条 socket 生命周期测例也会一起变红。

use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::store::session_config::{SessionConfig, SessionKind};
use super::{term, term_diag, term_lifecycle};

/// 本文件所有测例共用一把串行锁：交付队列 `take_deliveries()` 是**进程全局**的，
/// 两个测例并行会互相把对方的 transport 领走（领走又因 uid 失配丢弃 → 假泄漏）。
///
/// 同时给每条测例划一段**互不重叠的标签序号**：`NEXT_ID` 是 thread_local，
/// 而 cargo 一条测例一条线程 ⇒ 不加隔离的话每个测例的第一个标签 uid 都是 `tab-0`，
/// 上一个测例收尾时才发出的 `Exit` 会被下一个测例领走（详见 `term_lifecycle::reserve_tab_ids`）。
/// 32 个一格是留量：最长的一条（20 轮开→关）也占不满。
pub(super) fn net_serial() -> std::sync::MutexGuard<'static, ()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    static BLOCK: AtomicUsize = AtomicUsize::new(0);
    let guard = L
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Local-only fixtures also mint tab-0/tab-1. Their asynchronous Exit events
    // must not retire the first loopback peer while the next fixture is reading.
    term_lifecycle::reserve_tab_ids(BLOCK.fetch_add(1, Ordering::SeqCst) as u32 * 32 + 1_000_000);
    guard
}

/// 本机哑服务端：accept 后把 socket 留在库里供"是否仍打开"计数，永不回包。
pub(super) struct Sink {
    pub(super) port: u16,
    accepted: Arc<AtomicUsize>,
    pub(super) live: Arc<Mutex<Vec<TcpStream>>>,
}

impl Sink {
    pub(super) fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(Mutex::new(Vec::new()));
        let (a, l) = (accepted.clone(), live.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                // 非阻塞：稍后用「读到 0 字节 = 已关 / WouldBlock = 仍活」判定。
                let _ = stream.set_nonblocking(true);
                a.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut v) = l.lock() {
                    v.push(stream);
                }
            }
        });
        Ok(Self {
            port,
            accepted,
            live,
        })
    }

    pub(super) fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// 对端仍打开的 socket 数（泄漏预言机本体）。
    fn still_open(&self) -> usize {
        let Ok(mut v) = self.live.lock() else {
            return usize::MAX;
        };
        let mut buf = [0u8; 1];
        let mut open = 0usize;
        for stream in v.iter_mut() {
            match stream.read(&mut buf) {
                Ok(0) => {}
                Ok(_) => open += 1,
                Err(ref e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    open += 1;
                }
                Err(_) => {}
            }
        }
        open
    }
}

pub(super) fn tcp_cfg(port: u16) -> SessionConfig {
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    if let Some(t) = cfg.tcp.as_mut() {
        t.host = "127.0.0.1".into();
        t.port = port;
    }
    cfg
}

#[test]
fn explicit_sender_targets_do_not_expand_or_duplicate_when_sync_is_enabled() {
    use super::term_ops;
    let _serial = net_serial();
    let _input = super::term_tests::input_lock_guard();
    let sinks: Vec<_> = (0..3).map(|_| Sink::start().unwrap()).collect();
    let ids: Vec<_> = sinks
        .iter()
        .map(|s| term::open_tab_with_config(&tcp_cfg(s.port)))
        .collect();
    assert!(wait_until(Duration::from_secs(5), || {
        for &id in &ids {
            settle(id);
        }
        sinks.iter().all(|s| s.accepted() == 1)
            && ids.iter().all(|&id| term_diag::term_status_kind(id) == 2)
    }));
    let targets = [ids[0], ids[1], ids[0], u32::MAX];
    let sync = super::sync_channels_tests::fixture(&ids);
    // A payload beyond the FIFO's public memory budget must report no
    // admission for both encoded text and raw Hex bytes.
    let oversized = vec![b'x'; (1 << 20) + 1];
    for raw in [false, true] {
        assert_eq!(term_ops::term_write_sessions(&ids[..1], &oversized, raw), 0);
    }
    term_ops::term_set_locked(true);
    assert_eq!(
        term_ops::term_write_sessions(&targets, b"BLOCKED", false),
        0
    );
    term_ops::term_set_locked(false);
    assert_eq!(
        term_ops::term_write_sessions(&targets, b"GROUP_ONLY", false),
        2
    );
    drop(sync);
    let received = std::cell::RefCell::new(vec![Vec::new(); 3]);
    assert!(wait_until(Duration::from_secs(3), || {
        for (i, sink) in sinks.iter().enumerate() {
            let mut peers = sink.live.lock().unwrap();
            let mut bytes = [0; 128];
            if let Ok(n) = peers[0].read(&mut bytes) {
                received.borrow_mut()[i].extend_from_slice(&bytes[..n]);
            }
        }
        received.borrow()[0].len() >= 10 && received.borrow()[1].len() >= 10
    }));
    std::thread::sleep(Duration::from_millis(100));
    for (i, sink) in sinks.iter().enumerate() {
        let mut peers = sink.live.lock().unwrap();
        let mut bytes = [0; 128];
        if let Ok(n) = peers[0].read(&mut bytes) {
            received.borrow_mut()[i].extend_from_slice(&bytes[..n]);
        }
    }
    assert_eq!(
        &*received.borrow(),
        &[b"GROUP_ONLY".to_vec(), b"GROUP_ONLY".to_vec(), Vec::new()]
    );
    for id in ids {
        term::term_close_tab(id);
    }
}

pub(super) fn wait_until(deadline: Duration, f: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    loop {
        if f() {
            return true;
        }
        if t.elapsed() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 泵一次事件，把已建好的 transport 交给会话宿主。
///
/// `pump_all()` 里 `take_deliveries()` → `host.deliver(proto)` 是唯一交付路径；
/// 不泵则 `host.proto` 恒为 `None`，`disconnect()` 无物可关。
/// 这里**刻意不 `shared_runtime().enter()`**：与 C++ GUI 线程里的真实 `term_poll` 同处境。
/// R-17 未修之前这里需要自备 guard（当时靠它绕过 panic），修复后已撤掉 —— 留着会把
/// 回归藏起来，只有末条守卫变红，其余四条照绿。
pub(super) fn settle(tab: u32) {
    term::term_poll(tab);
}

/// SOAK-02 的壳层入口版：反复「开标签 → 关标签」，socket 计数必须是**锯齿**不是阶梯。
#[test]
fn closing_tcp_tabs_releases_every_socket_at_the_peer() {
    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    const CYCLES: usize = 20;

    for i in 0..CYCLES {
        let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
        assert!(
            wait_until(Duration::from_secs(5), || sink.accepted() > i),
            "第 {i} 轮标签没打到本机服务端（壳层 Tcp 会话没起来）"
        );
        settle(tab);
        term::term_close_tab(tab);
    }

    // `SessionHost::disconnect()` 是 fire-and-forget 到共享 runtime，给收敛时间。
    let drained = wait_until(Duration::from_secs(5), || sink.still_open() == 0);
    assert_eq!(
        sink.accepted(),
        CYCLES,
        "对端连接数与开关轮数不等 —— 有额外连接被建出来"
    );
    assert!(
        drained,
        "关标签后对端仍打开 {} 条 socket（{CYCLES} 轮开关之后应为 0）：命中 R-03",
        sink.still_open()
    );
}

/// 重连换 uid（`term_reconnect_tab` 会 generation+1 并 mint 新 uid）——
/// 上一代 transport 必须被关掉，否则每次重连都在服务端留一条孤儿连接。
#[test]
fn reconnecting_a_tab_does_not_orphan_the_previous_socket() {
    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    const CYCLES: usize = 10;

    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 1),
        "首个 Tcp 会话没起来"
    );
    settle(tab);
    for i in 0..CYCLES {
        assert!(term_lifecycle::term_reconnect_tab(tab), "重连返回 false");
        let want = i + 2;
        assert!(
            wait_until(Duration::from_secs(5), || sink.accepted() >= want),
            "第 {i} 次重连没产生新连接（对端计数 = {}）",
            sink.accepted()
        );
        settle(tab);
    }

    assert_eq!(
        sink.accepted(),
        CYCLES + 1,
        "连接数超出「首连 + 每轮重连」之和"
    );
    let only_current = wait_until(Duration::from_secs(5), || sink.still_open() <= 1);
    assert!(
        only_current,
        "重连 {} 次后对端仍打开 {} 条 socket（应只剩当前 1 条）：上一代 transport 没关",
        CYCLES,
        sink.still_open()
    );
    // R-01 的运行时取证仍需一条真 SFTP 通道（`sftp_open` → `open_sftp()`），
    // 本机哑服务端没有 SSH，拿不到 channel —— 不在此测内造假象，留给人工/授权靶机。
    term::term_close_tab(tab);
}

/// 关标签后对端 socket 必须关闭（状态码另见 R-19 守卫）。
#[test]
fn disconnecting_a_tcp_tab_closes_the_socket() {
    use crate::rust::term_diag::term_status_kind;

    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || {
            // Server accept can precede the asynchronous host delivery. Wait for
            // both the tracked peer and the client outcome within the same budget.
            settle(tab);
            sink.accepted() >= 1 && sink.still_open() == 1 && term_status_kind(tab) != 1
        }),
        "Tcp 会话未完成服务端接入和客户端交付"
    );
    let kind = term_status_kind(tab);
    assert!(
        matches!(kind, 2 | 3),
        "交付后仍不是「已交付且存活」（2 或 3），实得 {kind}，状态文案＝{:?} —— \
         5 表示 note 非空（被写成错误/状态文案），1 表示交付没发生",
        crate::rust::term_diag::term_status(tab)
    );
    // R-18 已修：`u8::from(local) + 2` ⇒ 本地 shell → 3=running，远程 → 2=connected。
    // Tcp 会话是远程，这里必须拿到 2（connected），不是 3。
    assert_eq!(
        kind, 2,
        "R-18：远程会话应报 2=connected（修复前方向相反报 3）"
    );
    assert!(term_lifecycle::term_disconnect_tab(tab));
    assert!(
        wait_until(Duration::from_secs(5), || sink.still_open() == 0),
        "disconnect 后对端 socket 没关（仍 {} 条）",
        sink.still_open()
    );
    term::term_close_tab(tab);
}

/// **R-19 回归守卫（已修，转常规门禁）**：
/// 手动「断开」后的状态类型码应为 4=disconnected。修复前实测恒为 5=error ——
/// `term_status_kind` 曾把任何非空 note 一律判 5，而 `term_disconnect_tab`
/// 恰恰把 note 写成「会话已断开」，于是 4 从菜单路径不可达，用户看到 ⚠ 异常。
/// 修复：note 的错误性由 `TermState::error` 标志承载（R-19 拆字段）。
#[test]
fn a_manually_disconnected_tab_reports_disconnected_not_error() {
    use crate::rust::term_diag::term_status_kind;

    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 1),
        "Tcp 会话没起来"
    );
    settle(tab);
    term_lifecycle::term_disconnect_tab(tab);
    assert_eq!(term_status_kind(tab), 4, "断开后应是 4=disconnected");
    term::term_close_tab(tab);
}

/// **R-17 回归守卫（修复后转绿，已并入常规门禁）**：
/// 在**没有** runtime guard 的线程上泵一次交付，应当正常完成而不是 panic。
/// 修复前实测 panic 于 `core/src/read_loop.rs:35`：`there is no reactor running`。
/// 前提自己也要断言：若哪天这条测例跑在带 runtime 上下文的线程上，它就永远绿着、失去意义。
#[test]
fn delivering_a_transport_does_not_require_the_pump_thread_to_hold_a_runtime_guard() {
    let _serial = net_serial();
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "本测例必须跑在完全没有 runtime 上下文的线程上，否则守卫形同虚设"
    );
    let sink = Sink::start().expect("本机 listener 起不来");
    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 1),
        "Tcp 会话没起来"
    );
    // 故意不 enter()：这就是 C++ GUI 线程里 term_poll 的真实处境。
    term::term_poll(tab);
    // 泵完之后必须"真的交付到了"，否则这条守卫什么也没测到：
    // `term_status_kind` 在 `host.connected()`（= `proto` 已被交付）之前恒为 1=connecting。
    // 取 `!= 1` 而不写死 2/3，避免与 R-18 的类型码方向纠缠在一起。
    assert_ne!(
        term_diag::term_status_kind(tab),
        1,
        "泵完之后仍停在 connecting ⇒ transport 没被交付，守卫空转"
    );
    term::term_close_tab(tab);
}

/// **R-03 的 socket 面：负结果**。连接已完成但还没被泵交付时就关标签，随后任何一次泵
/// 会把这个滞留 uid 的 transport 领走、因失配直接 drop —— 实测**对端 socket 仍然关闭**
/// （`TcpInstance` 的 mpsc sender 随 Arc drop，读任务 send 失败即退出并释放 reader）。
/// 所以「drop 不调 disconnect()」在 TCP 传输上不构成 socket 泄漏；R-03 剩下的
/// 未证面只有 SSH（`ssh2::Session` + 独立 blocking 读线程，不走这条 mpsc 退出路径）。
#[test]
fn closing_a_tab_before_delivery_still_closes_its_socket() {
    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 1),
        "Tcp 会话没起来"
    );
    term::term_close_tab(tab);
    // 再开一个标签并泵它：把上一个 uid 的滞留交付领走（触发 uid 失配 drop）。
    let other = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 2),
        "第二个 Tcp 会话没起来"
    );
    settle(other);
    term::term_close_tab(other);
    assert!(
        wait_until(Duration::from_secs(5), || sink.still_open() == 0),
        "有 transport 被 drop 却没关 socket（对端仍 {} 条）",
        sink.still_open()
    );
}

/// **R-10 回归守卫**：快速连点/竞态下同一标签可能被关两次、或在关闭后仍被泵几帧。
/// 修复前 `term_close_tab` 走 `.expect("tab id 不存在")`，panic 跨 cxx =
/// 0xC0000409 整进程消失（SOAK-04 的判据场景）。修复后：幂等关 + 迟到 poll 无害。
#[test]
fn closing_a_tab_twice_and_a_late_poll_do_not_panic() {
    let _serial = net_serial();
    let sink = Sink::start().expect("本机 listener 起不来");
    let tab = term::open_tab_with_config(&tcp_cfg(sink.port));
    assert!(
        wait_until(Duration::from_secs(5), || sink.accepted() >= 1),
        "Tcp 会话没起来"
    );
    settle(tab);
    let left = term::term_close_tab(tab);
    // 第二次关同一 id：幂等返回剩余数，不 panic。
    assert_eq!(term::term_close_tab(tab), left, "重复关闭应幂等");
    // 标签已关但 C++ view 还没销毁时的最后几帧 poll：无重绘，不 panic。
    assert!(!term::term_poll(tab), "迟到 poll 应返回无重绘");
    assert_eq!(term::term_tab_count(), left);
}
