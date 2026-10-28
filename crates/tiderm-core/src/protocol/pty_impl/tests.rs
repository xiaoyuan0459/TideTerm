use super::*;

#[test]
fn resolve_shell_auto_returns_powershell_on_windows() {
    let (program, _) = shell::resolve("auto").expect("resolve");
    if cfg!(target_os = "windows") {
        assert!(program.contains("powershell"));
    } else {
        assert!(!program.is_empty());
    }
}

#[test]
fn resolve_shell_unknown_returns_err() {
    let result = shell::resolve("ksh");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("unknown shell: ksh"));
}

/// R-14：Windows 下 bash 必须优先解析到 Git Bash 全路径 —— 裸名会被
/// CreateProcess 搜索序解析到 System32\bash.exe（WSL），冷启动数秒零输出。
/// zsh/fish 在 Windows 只加 `-i`；Unix 下 PTY 即交互态、必须无参。
#[test]
fn resolve_shell_posix_shells_are_interactive_only_on_windows() {
    let (bash_prog, bash_args) = shell::resolve("bash").expect("resolve");
    for shell in ["zsh", "fish"] {
        let (program, args) = super::shell::resolve(shell).expect("resolve");
        assert_eq!(program, shell);
        if cfg!(target_os = "windows") {
            assert_eq!(args, vec!["-i".to_string()]);
        } else {
            assert!(args.is_empty());
        }
    }
    if !cfg!(target_os = "windows") {
        assert_eq!(bash_prog, "bash");
        assert!(bash_args.is_empty());
        return;
    }
    match shell::git_bash_path() {
        Some(path) => {
            assert_eq!(bash_prog, path);
            assert!(std::path::Path::new(&bash_prog).is_file());
            assert_eq!(bash_args, vec!["--login".to_string(), "-i".to_string()]);
        }
        None => {
            assert_eq!(bash_prog, "bash");
            assert_eq!(bash_args, vec!["-i".to_string()]);
        }
    }
}

/// PERF-06：按键 → 回显延迟。口径 = `write()` 进 PTY 到读回该次输出的第一批字节，
/// **不含** UI 输入处理与字形光栅化（渲染段另见 `native --g1-bench`）。
///
/// 读侧只允许一个永不放弃的 pump 任务：`Self::read()` 把阻塞读放进
/// `spawn_blocking` 并持有同一把 `AsyncMutex`，一旦调用方用 `timeout` 放弃 future，
/// 那个 blocking 任务仍会吃掉下一批字节并让后续读排队 —— 时间戳会被偷走。
/// 时间戳由 pump 线程在 `read()` 返回瞬间打点，故轮询只影响「发现」不影响「测量」精度。
#[tokio::test]
#[ignore = "起真实 PTY + 本地 shell，秒级耗时，不进普通 CI"]
async fn pty_echo_latency_percentiles() {
    const N: usize = 200;
    let shell = std::env::var("TT_LAT_SHELL").unwrap_or_else(|_| "auto".into());
    let pty = Arc::new(PtyInstance::spawn(80, 24, &shell).expect("spawn pty"));
    let clock = Arc::new(std::time::Instant::now());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let (pump_pty, pump_clock) = (pty.clone(), clock.clone());
    tokio::spawn(async move {
        loop {
            match pump_pty.read().await {
                Ok(b) if !b.is_empty() => {
                    let t = pump_clock.elapsed().as_micros() as u64;
                    if tx.send(t).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    // 等横幅/提示符吐完再开始，随后丢弃所有积压事件。
    tokio::time::sleep(Duration::from_millis(1500)).await;
    pty.write(b"\r").await.expect("warmup write");
    tokio::time::sleep(Duration::from_millis(300)).await;
    while rx.try_recv().is_ok() {}

    let mut samples = Vec::with_capacity(N);
    let mut silent = 0usize;
    for i in 0..N {
        while rx.try_recv().is_ok() {}
        let w = clock.elapsed().as_micros() as u64;
        pty.write(b"z").await.expect("write");
        let mut got = None;
        let until = std::time::Instant::now() + Duration::from_millis(1000);
        while got.is_none() && std::time::Instant::now() < until {
            match rx.try_recv() {
                Ok(t) => got = Some(t),
                Err(_) => tokio::time::sleep(Duration::from_micros(200)).await,
            }
        }
        match got {
            Some(t) => samples.push(t - w),
            None => silent += 1,
        }
        if i == 0 && samples.is_empty() {
            panic!("shell={shell} 首键 1s 内无任何输出 —— 会话不可用，延迟判据无意义");
        }
        let _ = pty.write(b"\x08").await; // 退格，防输入行无限增长
    }

    assert!(!samples.is_empty(), "全部 {N} 次按键均无输出");
    samples.sort_unstable();
    let pct = |p: usize| samples[(samples.len() * p / 100).min(samples.len() - 1)];
    println!(
        "[PERF-06] shell={shell} n={}/{} min={}us p50={}us p95={}us p99={}us max={}us 无输出={}",
        samples.len(),
        N,
        samples[0],
        pct(50),
        pct(95),
        pct(99),
        samples[samples.len() - 1],
        silent
    );
    assert!(
        pct(50) < 5_000,
        "P50 {}us 超过 5ms —— 应用侧 PTY 往返本身就不达标",
        pct(50)
    );
}

#[test]
fn spawn_creates_alive_session() {
    let shell = if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    };
    let pty = PtyInstance::spawn(80, 24, shell).expect("spawn");
    assert!(pty.is_alive());
}

#[tokio::test]
async fn write_and_read_round_trip() {
    let shell = if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    };
    let pty = PtyInstance::spawn(80, 24, shell).expect("spawn");

    // Give the shell a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Write a simple command
    pty.write(b"echo hello\r\n").await.expect("write");

    // Read output (should contain "hello" somewhere)
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let data = pty.read().await.expect("read");
    assert!(!data.is_empty());
}

#[tokio::test]
async fn resize_succeeds() {
    let shell = if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    };
    let pty = PtyInstance::spawn(80, 24, shell).expect("spawn");
    pty.resize(120, 40).await.expect("resize");
}

#[tokio::test]
async fn disconnect_stops_shell_and_is_idempotent() {
    let shell = if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    };
    let pty = PtyInstance::spawn(80, 24, shell).expect("spawn");
    pty.disconnect().await.expect("disconnect");
    assert!(!pty.is_alive());
    pty.disconnect().await.expect("second disconnect");
    assert!(pty.write(b"discarded input").await.is_err());
}

/// A shell without prompt integration still supplies its real directory, and
/// disconnect retires that snapshot instead of leaking it into another session.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn working_directory_follows_plain_shell_and_retires_on_disconnect() {
    let fixture = tempfile::tempdir().expect("fixture");
    let initial = fixture.path().join("initial");
    let target = fixture.path().join("directory space 中文 'quoted'");
    std::fs::create_dir(&initial).expect("initial directory");
    std::fs::create_dir(&target).expect("target directory");
    let pty = PtyInstance::spawn_with_options(
        80,
        24,
        "bash",
        &["--noprofile".into(), "--norc".into(), "-i".into()],
        Some(initial.to_str().expect("UTF-8 fixture")),
        "xterm-256color",
    )
    .expect("plain shell");
    let observed_initial = wait_for_directory(&pty, &initial).await;
    let quoted = target
        .to_str()
        .expect("UTF-8 fixture")
        .replace('\'', "'\"'\"'");
    pty.write(format!("unset HISTFILE; cd -- '{quoted}'\r").as_bytes())
        .await
        .expect("change directory");
    let observed_target = wait_for_directory(&pty, &target).await;
    pty.disconnect().await.expect("disconnect");
    assert!(
        observed_initial,
        "initial process directory was unavailable"
    );
    assert!(
        observed_target,
        "plain shell's Unicode directory did not follow cd"
    );
    assert!(
        pty.local_working_directory().is_none(),
        "disconnected directory remained available"
    );
}

#[cfg(target_os = "linux")]
async fn wait_for_directory(pty: &PtyInstance, expected: &std::path::Path) -> bool {
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < until {
        if pty.local_working_directory().as_deref() == Some(expected) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}
