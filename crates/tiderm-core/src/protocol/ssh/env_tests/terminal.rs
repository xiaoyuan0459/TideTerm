//! Live terminal echo, exit and resize contracts.
use super::*;

/// CON-03/09：密码认证 → 命令回显 → 正常退出。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS"]
async fn real_ssh_password_echo_and_exit() {
    let (host, port, user, pass) = env_all();
    let c = SshConfig {
        host,
        port,
        username: user,
        auth: SshAuth::Password { password: pass },
        connect_timeout_secs: 15,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 0,
        link_watchdog_secs: 0,
        options: Default::default(),
        term: "xterm-256color".into(),
    };
    let s = SshInstance::connect(c, 120, 32, "real1".into(), &AutoAccept, Arc::new(NoKb))
        .await
        .expect("真实 SSH 连接失败");
    let mut acc = String::new();
    wait_prompt(&s, &mut acc).await;
    let marker = format!("TTMARK_{}", std::process::id());
    s.write(format!("printf '%s\\n' {marker}\r").as_bytes())
        .await
        .expect("write");
    let deadline = Instant::now() + Duration::from_secs(15);
    // 命令回显里就含标记本身，只出现一次不算执行过（首版 CON-03 就假绿在这里）。
    while Instant::now() < deadline && acc.matches(&marker).count() < 2 {
        if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
            acc.push_str(&String::from_utf8_lossy(&b));
        }
    }
    assert!(
        acc.matches(&marker).count() >= 2,
        "15s 内只看到回显、没看到真实输出：{acc:?}"
    );
    println!("[CON-03] echo ok, bytes_seen={}", acc.len());
    s.write("exit\r".as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!(
        "[CON-09] alive_after_exit={} exit_code={:?}",
        s.is_alive(),
        s.exit_code()
    );
    s.disconnect().await.expect("disconnect");
}

/// TTY-04/RESIZE-01：resize 在真实 PTY 上生效（`stty size` 随动）。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS"]
async fn real_ssh_resize_propagates() {
    let (host, port, user, pass) = env_all();
    let c = SshConfig {
        host,
        port,
        username: user,
        auth: SshAuth::Password { password: pass },
        connect_timeout_secs: 15,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 0,
        link_watchdog_secs: 0,
        options: Default::default(),
        term: "xterm-256color".into(),
    };
    let s = SshInstance::connect(c, 80, 24, "real2".into(), &AutoAccept, Arc::new(NoKb))
        .await
        .expect("连接失败");
    // Ubuntu 的登录横幅有 1 KB+，未等提示符就写输入会被吞掉（首版就栽在这里）。
    let mut acc = String::new();
    wait_prompt(&s, &mut acc).await;
    assert!(
        acc.contains('$') || acc.contains('#'),
        "10s 内没等到 shell 提示符，无法判定 resize：{acc:?}"
    );

    s.resize(140, 44).await.expect("resize");
    let done = format!("TTDONE_{}", std::process::id());
    s.write(format!("stty size; printf '%s\\n' {done}\r").as_bytes())
        .await
        .expect("write");
    let until = Instant::now() + Duration::from_secs(15);
    // 回显里就含标记本身（命令行的最后一段），所以必须等它出现**两次**：
    // 第一次是 PTY 回显的输入，第二次才是 printf 的真实输出。
    while Instant::now() < until && acc.matches(&done).count() < 2 {
        if let Ok(Ok(b)) = tokio::time::timeout(Duration::from_secs(2), s.read()).await {
            acc.push_str(&String::from_utf8_lossy(&b));
        }
    }
    assert!(
        acc.matches(&done).count() >= 2,
        "15s 内未看到命令真实输出：{acc:?}"
    );
    // 取「第二次出现」之前的文本，才是 stty 的输出所在。
    let at = acc.match_indices(&done).nth(1).map(|(i, _)| i).unwrap_or(0);
    let tail: String = acc[..at]
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    println!("[RESIZE-01] 80x24 → resize(140,44)，stty 输出前 40 字节 = {tail:?}");
    assert!(
        tail.contains("44 140"),
        "resize 未传导到远端 PTY（期望 stty size 报 `44 140`，实得 {tail:?}）"
    );
    s.disconnect().await.unwrap();
}

/// RENDER-01 / R-11：大输出经 SSH PTY 管道回传，**一个字节都不能丢**。
/// R-11 的静默丢字节只有在洪水式输出下才暴露，故用 300 000 行确定性序列比对总长。
#[tokio::test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS；传输数 MB"]
async fn real_ssh_flood_loses_no_bytes() {
    const LINES: u64 = 300_000;
    let s = open_once("flood").await.expect("真实 SSH 连接失败");
    let mut acc = String::new();
    wait_prompt(&s, &mut acc).await;

    // 期望字节数：每行 = 数字位数 + 1 个 \n
    let expected: u64 = (1..=LINES).map(|n| n.to_string().len() as u64 + 1).sum();
    let start = "TTSTART";
    let end = "TTEND";
    s.write(format!("printf '%s\\n' {start}; seq 1 {LINES}; printf '%s\\n' {end}\r").as_bytes())
        .await
        .unwrap();

    let until = Instant::now() + Duration::from_secs(120);
    // 需要 end 的第 2 次出现（第 1 次是命令自身的回显）。
    while Instant::now() < until && acc.matches(end).count() < 2 {
        match tokio::time::timeout(Duration::from_secs(3), s.read()).await {
            Ok(Ok(b)) => acc.push_str(&String::from_utf8_lossy(&b)),
            Ok(Err(e)) => panic!("read: {e}"),
            Err(_) => continue,
        }
    }
    let starts: Vec<usize> = acc
        .match_indices(start)
        .skip_while(|_| false)
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = acc.match_indices(end).map(|(i, _)| i).collect();
    assert!(
        starts.len() >= 2 && ends.len() >= 2,
        "未采到起止标记（starts={starts:?} ends={ends:?}）"
    );
    // 真实输出 = 第 2 个 start 之后 到 第 1 个「出现在其后的」 end 之前。
    let lo = starts[1] + start.len();
    let hi = *ends
        .iter()
        .find(|e| **e > lo)
        .expect("start 之后没有 end 标记");
    let body: String = acc[lo..hi].chars().filter(|c| *c != '\r').collect();
    let body = body.trim_start_matches(['\n', ' ']);
    let got = body.len() as u64;

    println!(
        "[RENDER-01] 期望 {expected} 字节，收到 {got} 字节，差额 {}；末行={:?}",
        expected as i64 - got as i64,
        body.lines().next_back().unwrap_or("<空>")
    );
    assert_eq!(
        got, expected,
        "大输出丢/多字节（期望 {expected}，实得 {got}）—— 命中 R-11 静默丢包"
    );
    assert!(
        body.lines().next_back() == Some("300000"),
        "末行不是 300000，序列被截断"
    );
    s.disconnect().await.unwrap();
}
