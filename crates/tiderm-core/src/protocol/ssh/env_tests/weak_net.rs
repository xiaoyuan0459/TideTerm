use super::*;

// ── 弱网 / 异常网络注入（NET-01~03）─────────────────────────────────────────
//
// 本机没有 `tc`，也不在靶机上装任何东西，所以弱网用**应用层哑管道**造：
// 监听 127.0.0.1:0，把字节原样转发给真实 SSH 端口，期间可注入
//   ① 单向固定延迟（`TT_LAT_MS`，缺省 150）—— 语义等价 `tc netem delay <L>ms`
//      **不带 rate**：每个块只在自己的方向上晚到 L，块之间不串行等待，
//      故 RTT ≈ 2L 而吞吐不受限（一次请求-响应 = 2L + 服务端耗时）；
//   ② 黑洞 —— 只停转发、不关 socket（等价拔线后中间设备静默丢包 / NAT 表项过期）；
//      开启时连同**在途未投递**的块一起丢，否则「黑洞」会漏出尾巴；
//   ③ 硬切断 —— 立即关闭两端（FIN。本 Rust 版本的 `TcpStream::set_linger` 仍不稳定，
//      所以发不出 RST；FIN 已足以检验「链路消失后客户端是否得知」）。
// 局限要写清（别拿它当 netem 的等价物）：
//   * 无**包级丢包/抖动**（30% loss 与真丢包仍需 WSL 或跳板机上的 tc）；
//   * 无**带宽上限**，因此不建模 BDP —— 真 2000 ms 链路上拥塞窗口会掐住吞吐，这里不会。
//     它测的是**往返延迟-bound 的交互**（握手、按键回显、命令往返），恰是「弱网功能可用性」的口径。

/// 单向延迟（ms）。`TT_LAT_MS=1000` → RTT ≈ 2000 ms，即目标点名的「高延迟 2000ms」。
fn lat_ms() -> u64 {
    std::env::var("TT_LAT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150)
}

/// 起一条只服务单个客户端的哑管道。返回 `(本地端口, 切断开关, 黑洞开关)`。
fn weak_pipe(
    upstream: &str,
    upstream_port: u16,
    latency: Duration,
) -> std::io::Result<(u16, Arc<AtomicBool>, Arc<AtomicBool>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let host = upstream.to_string();
    let cut = Arc::new(AtomicBool::new(false));
    let hole = Arc::new(AtomicBool::new(false));
    let (sc, sh) = (cut.clone(), hole.clone());
    std::thread::spawn(move || loop {
        let Ok((client, _)) = listener.accept() else {
            return;
        };
        let host = host.clone();
        let (c1, h1) = (sc.clone(), sh.clone());
        std::thread::spawn(move || {
            let Ok(upstream) = TcpStream::connect((host.as_str(), upstream_port)) else {
                return;
            };
            let Ok(up_w) = upstream.try_clone() else {
                return;
            };
            let Ok(cl_w) = client.try_clone() else { return };
            let _ = client.set_read_timeout(Some(Duration::from_millis(20)));
            let _ = upstream.set_read_timeout(Some(Duration::from_millis(20)));

            let pump = |mut src: TcpStream,
                        mut dst: TcpStream,
                        cut: Arc<AtomicBool>,
                        hole: Arc<AtomicBool>| {
                move || {
                    let mut buf = [0u8; 8192];
                    // 在途队列：(到期时刻, 字节)。读侧不 sleep，延迟才不会把吞吐串行化
                    // —— 每块都睡 latency 的话，1000 ms 档会把一次握手拖成几十倍。
                    let mut inflight: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();
                    loop {
                        if cut.load(Ordering::SeqCst) {
                            break; // 切断：在途一起丢，等价链路当场消失
                        }
                        if hole.load(Ordering::SeqCst) {
                            inflight.clear();
                        } else {
                            while !inflight.is_empty() && inflight[0].0 <= Instant::now() {
                                if let Some((_, data)) = inflight.pop_front()
                                    && dst.write_all(&data).is_err()
                                {
                                    return;
                                }
                            }
                        }
                        match src.read(&mut buf) {
                            Ok(0) => {
                                // EOF：按序把在途投完再关，否则握手的尾包会被吃掉
                                while let Some((_, data)) = inflight.pop_front() {
                                    if dst.write_all(&data).is_err() {
                                        return;
                                    }
                                }
                                break;
                            }
                            Ok(n) => {
                                if !hole.load(Ordering::SeqCst) {
                                    let at = Instant::now() + latency;
                                    inflight.push_back((at, buf[..n].to_vec()));
                                }
                            }
                            Err(ref e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut => {}
                            Err(_) => break,
                        }
                    }
                }
            };

            let a = std::thread::spawn(pump(client, up_w, c1.clone(), h1.clone()));
            let b = std::thread::spawn(pump(upstream, cl_w, c1, h1));
            let _ = a.join();
            let _ = b.join();
            // 两个 pump 退出后 4 个句柄全部 drop → 两端关闭（FIN）。
        });
    });

    Ok((port, cut, hole))
}

/// 发一条命令并等它**真实执行**：标记须出现 ≥2 次（回显 1 次 + 输出 1 次），
/// 否则终端回显就会先把判据骗过去（见 §7 教训 3）。
async fn ran_marker(s: &SshInstance, acc: &mut String, marker: &str, secs: u64) -> bool {
    s.write(format!("printf '%s\\n' {marker}\r").as_bytes())
        .await
        .expect("write");
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
            acc.push_str(&String::from_utf8_lossy(&b));
            if acc.matches(marker).count() >= 2 {
                return true;
            }
        }
    }
    false
}

/// NET-01：高延迟链路上能否建连 —— 先按**出厂默认** 15 s 超时试一次，再放宽重试。
/// 单向延迟由 `TT_LAT_MS` 给定（RTT ≈ 2×）；目标点名的「高延迟 2000 ms」＝ `TT_LAT_MS=1000`。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS，并会向真实 SSH 端口起本机代理"]
async fn weak_net_high_latency_handshake() {
    let (host, port, _, _) = env_all();
    let lat = lat_ms();
    let (proxy, _cut, _hole) = weak_pipe(&host, port, Duration::from_millis(lat)).expect("bind");

    let t = Instant::now();
    let tight = open_via_proxy("net01-tight", proxy, 15).await;
    let tight_ms = t.elapsed().as_millis();
    let (ok_default, echo_ms) = match &tight {
        Some(s) => {
            let mut acc = String::new();
            wait_prompt(s, &mut acc).await;
            let t2 = Instant::now();
            let m = format!("TTNET_{}", std::process::id());
            let ran = ran_marker(s, &mut acc, &m, 40).await;
            (ran, t2.elapsed().as_millis())
        }
        None => (false, 0),
    };
    drop(tight);

    let t = Instant::now();
    let loose = open_via_proxy("net01-loose", proxy, 60).await;
    let loose_ms = t.elapsed().as_millis();
    let mut ok_loose = false;
    let mut key_echo_ms = 0;
    if let Some(s) = loose {
        let mut acc = String::new();
        wait_prompt(&s, &mut acc).await;
        ok_loose = ran_marker(&s, &mut acc, "TTNET2", 60).await;
        // 单键回显：远端 PTY 处于 echo 模式，敲一个字符就该原样回来 ——
        // 弱网下用户感知的「打字粘不粘」，理论下界 = 2× 单向延迟。
        // 必须放在最后：裸字符会把提示符行弄脏（下一句变成 `xprintf …` → command not
        // found），先跑它会把「能跑」误判成不能跑。
        let base = acc.len();
        s.write(b"x").await.expect("write key");
        let t3 = Instant::now();
        let until = t3 + Duration::from_secs(30);
        while Instant::now() < until && !(acc.len() > base && acc.ends_with('x')) {
            if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
                acc.push_str(&String::from_utf8_lossy(&b));
            }
        }
        key_echo_ms = t3.elapsed().as_millis();
        ok_loose = ok_loose && acc.ends_with('x');
        s.disconnect().await.unwrap();
    }

    println!(
        "[NET-01] 单向 +{lat}ms（RTT≈{}ms）：默认 15s 超时建连 ok={ok_default} 耗时={tight_ms}ms 命令往返={echo_ms}ms；放宽到 60s ok={ok_loose} 耗时={loose_ms}ms 单键回显={key_echo_ms}ms",
        lat * 2
    );
    assert!(
        ok_loose,
        "放宽超时后仍无法在 {lat}ms 单向延迟的链路上建连并执行命令 —— 弱网不可用（不是超时配置问题）"
    );
}

/// CON-11b / **R-20 哨兵**：`connect_timeout_secs` 到底管多大一段？
///
/// 期望它约束**整条**建连（transport handshake + 认证 + 开 PTY）；修复前只管
/// 第 1 步 —— `connection.rs` 的 `tokio::time::timeout` 只包住 `connect_handshake`，
/// 其后的 host-key 校验、认证、开 PTY 是裸 `spawn_blocking().await`，**无任何
/// 超时**。实测（RTT≈2 s 哑管道，预算 15 s）：`connect()` 在 16 439/16 512 ms
/// 后**成功**返回 —— 超时值被越过仍报成功。
///
/// **2026-09-24 修复已落地**：TCP/握手/认证/开 PTY 全部纳入预算（`run_bounded`），
/// 本用例应转绿；因需真实 SSH 服务端造出「握手快、认证慢」的链路，端到端
/// 确认待靶机恢复后执行（`-- --ignored` + `TEST_SSH_*`）。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS，并向真实 SSH 端口起本机代理（RTT≈2 s，约 20 s）"]
async fn connect_timeout_bounds_full_ssh_setup() {
    let (host, port, _, _) = env_all();
    // 延迟写死在本用例里、不读 TT_LAT_MS：哨兵得自己造出
    // 「握手落在预算内、认证+开 PTY 落在预算外」的慢链路。
    let (proxy, _cut, _hole) = weak_pipe(&host, port, Duration::from_millis(1_000)).expect("bind");
    const BUDGET: u64 = 15;

    let t = Instant::now();
    let s = open_via_proxy("net01b", proxy, BUDGET).await;
    let ms = t.elapsed().as_millis();
    match s {
        Some(s) => {
            s.disconnect().await.unwrap();
            panic!(
                "[R-20] 配置 connect_timeout={BUDGET}s，实际 {ms}ms 后**成功**建连 —— \
                 超时没管住认证/开 PTY 段"
            );
        }
        None => {
            let bound = BUDGET as u128 * 1000 + 1_500;
            assert!(
                ms >= (BUDGET - 2) as u128 * 1000,
                "[R-20] {ms}ms 即失败，未进入预期的超时阶段"
            );
            assert!(ms <= bound, "[R-20] 超时路径本身不受界：{ms}ms > {bound}ms");
            println!("[R-20] 本次在 {BUDGET}s 预算内失败返回（{ms}ms）—— 未复现，见注释");
        }
    }
}

/// NET-02：链路被关闭（代理两端同时 drop，FIN）后，客户端应在**有限时间**内得知。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS，并会向真实 SSH 端口起本机代理"]
async fn weak_net_forced_cut_is_detected() {
    let (host, port, _, _) = env_all();
    let (proxy, cut, _hole) = weak_pipe(&host, port, Duration::ZERO).expect("bind");
    let s = open_via_proxy("net02", proxy, 30)
        .await
        .expect("无法经代理建连，不能验证强制断链");
    let mut acc = String::new();
    wait_prompt(&s, &mut acc).await;
    assert!(
        ran_marker(&s, &mut acc, "TTCUT1", 20).await,
        "切断前会话本身就不通 —— 该用例无意义"
    );

    let t = Instant::now();
    cut.store(true, Ordering::SeqCst);
    let mut detected = None;
    while t.elapsed() < Duration::from_secs(20) {
        if !s.is_alive() {
            detected = Some(t.elapsed().as_millis());
            break;
        }
        // 也要能被 read() 得知：任一即可。
        if let Ok(Err(_)) = tokio::time::timeout(Duration::from_millis(500), s.read()).await {
            detected = Some(t.elapsed().as_millis());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!(
        "[NET-02] 链路关闭（FIN）后 {:?} ms 得知（is_alive={})",
        detected,
        s.is_alive()
    );
    assert!(
        detected.is_some(),
        "链路已关闭，20 s 内客户端仍认为会话存活 —— 用户界面会一直显示「已连接」"
    );
}

/// NET-03：静默黑洞（不关 socket、只丢包）后能否得知。
///
/// 修复前这条**永远 FAIL**：ssh 层无 keepalive，读线程把 `WouldBlock`/`TimedOut` 当
/// 「再睡 2 ms 重试」，于是 `is_alive()` 恒为 true。R-16 的修法只能在应用层 ——
/// 本仓的 `ssh2` 0.9 **没有** `keepalive_send` / `send_ignore` / `keepalive_config`
/// （2026-09-23 编译探针逐一确认 E0599），主动探活不可行；而"空闲"与"黑洞"在 API 层面
/// 同样是"read 一直 TimedOut"。唯一可区分的信号是**有键入却没有任何回字节**，
/// 即 `SshConfig::link_watchdog_secs`（默认 120 s，本用例拧到 5 s 让观察窗跑得完）。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS；跑满约 40 s"]
async fn weak_net_blackhole_is_detected() {
    let (host, port, user, pass) = env_all();
    let (proxy, _cut, hole) = weak_pipe(&host, port, Duration::ZERO).expect("bind");
    // 打开静默链路看门狗，拧到 5 s：30 s 观察窗内能判完，又不会误伤健康会话。
    let mut cfg = cfg_on("127.0.0.1".into(), proxy, user, pass, 30);
    cfg.link_watchdog_secs = 5;
    let s = SshInstance::connect(cfg, 80, 24, "net03".into(), &AutoAccept, Arc::new(NoKb))
        .await
        .expect("无法经代理建连，不能验证静默黑洞");
    let mut acc = String::new();
    wait_prompt(&s, &mut acc).await;
    assert!(
        ran_marker(&s, &mut acc, "TTBH1", 20).await,
        "注入前会话就不通 —— 该用例无意义"
    );

    hole.store(true, Ordering::SeqCst);
    let t = Instant::now();
    let mut detected = None;
    // 30 s 观察窗：期间持续按键（真实用户行为），看客户端会不会自己发现链路已死。
    let mut k = 0u32;
    while t.elapsed() < Duration::from_secs(30) {
        k += 1;
        let _ = s.write(b"\r").await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        if !s.is_alive() {
            detected = Some(t.elapsed().as_millis());
            break;
        }
    }
    let bytes_read = acc.len();
    println!(
        "[NET-03] 静默黑洞 {} s、按键 {k} 次后：得知={detected:?}（判定窗口 30 s），此前累计输出 {bytes_read} 字节",
        t.elapsed().as_secs()
    );
    assert!(
        detected.is_some(),
        "链路静默丢包（socket 未关）时，30 s 内客户端完全无法得知，`is_alive()` 恒 true —— \
         静默链路看门狗没有把「有键入却零回字节」判死（R-16）"
    );
}
