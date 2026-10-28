//! 壳层 **真 SSH 会话**的端到端旅程（CON-01/05/06 + TTY-01 + PERF-06 的网络段）。
//!
//! 这条路的价值在于它是**R-17 修复的直接证据**：修复之前，远程会话在第一次
//! `term_poll` 交付时就 panic（`deliver()` 里裸 `tokio::spawn` 没有 runtime 上下文），
//! 所以壳层的远程路径**从来没有被任何测例跑到过第二帧**。
//! 现在它跑通了：开标签 → 主机密钥确认 → 交付 → 提示符上屏 → 敲命令 → 屏幕上看到回显与输出。
//!
//! 凭据只从 `TEST_SSH_HOST/USER/PASS`（可选 `TEST_SSH_PORT`）读取。
//! 显式运行此 ignored 用例时，缺少环境变量或主机未预先受信都必须失败。
//! 测试绝不接受未知主机密钥，避免修改开发者的 known_hosts。

use std::time::{Duration, Instant};

use super::events;
use super::store::session_config::{SessionConfig, SessionKind, SshAuth};
use super::term_net_tests::{net_serial, settle, wait_until};
use super::term_tests::input_lock_guard;
use super::{term, term_diag, term_lifecycle};

/// Core is a normal dependency here, so its cfg(test) environment hook is absent.
/// Reuse the explicit self-test profile instead of a user's trust file.
pub(super) fn isolate_known_hosts() {
    static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    READY.get_or_init(|| {
        let fixture = std::path::PathBuf::from(
            std::env::var_os("TEST_SSH_KNOWN_HOSTS").expect("需要隔离 TEST_SSH_KNOWN_HOSTS"),
        );
        let fixture = fixture
            .canonicalize()
            .expect("known-hosts fixture must exist");
        assert!(fixture.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        let directory = fixture.parent().unwrap().join("native-ssh-profile");
        tiderm_core::set_config_directory(directory.clone()).expect("isolated SSH profile");
        std::fs::copy(fixture, directory.join("known_hosts")).expect("copy controlled host key");
    });
}

/// 从环境变量拼一个 SSH 会话配置；显式运行时缺少配置是测试失败。
pub(super) fn ssh_cfg_from_env() -> (SessionConfig, String) {
    isolate_known_hosts();
    let host = std::env::var("TEST_SSH_HOST").expect("需要 TEST_SSH_HOST");
    let user = std::env::var("TEST_SSH_USER").expect("需要 TEST_SSH_USER");
    let pass = std::env::var("TEST_SSH_PASS").expect("需要 TEST_SSH_PASS");
    let port = std::env::var("TEST_SSH_PORT")
        .ok()
        .map(|p| p.parse().expect("TEST_SSH_PORT 必须是有效端口"))
        .unwrap_or(22);
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    {
        let ssh = cfg.ssh.as_mut().expect("blank(Ssh) 自带 SshConfig 默认值");
        ssh.host = host.clone();
        ssh.port = port;
        ssh.username = user;
        ssh.auth = SshAuth::Password;
        ssh.password = pass;
        ssh.connect_timeout_secs = 25;
    }
    // A controlled fixture may pin the host-key algorithm whose public key was verified.
    cfg.meta.options.ssh.host_keys =
        std::env::var("TEST_SSH_HOST_KEY_ALGORITHMS").unwrap_or_default();
    (cfg, format!("{host}:{port}"))
}

/// CON-01/05/06 + TTY-01：**真 SSH 会话经 TideTerm 自己的壳层跑完整一趟**。
///
/// 判据取用户视角的四件事，全部走桥面可观测量（不看内部字段）：
/// ① 建连能到「已连接」（未知主机密钥时，壳层确实把确认提示交了出来 —— CON-06）；
/// ② 远端提示符出现在屏幕上；
/// ③ 敲进去的命令**原样回显**、跑完**出输出**（标记数两次：回显 + 输出，
///    否则 PTY 的回显会先把判据骗过去 —— 见 §7 教训 3）；
/// ④ 断开与关标签不 panic。
///
/// 顺带产出两个真机数字：建连+首帧、命令往返（壳层口径，含渲染），供 §4 取值。
/// 其中**命令往返还兼任 R-21 哨兵**：修掉 SSH 交互延迟前，这条测例在 `--ignored`
/// 下按预期 FAIL（断言里带证据）；修好后转绿。
#[test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS；会真连测试机"]
fn real_ssh_session_round_trips_through_the_shell_layer() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let (cfg, target) = ssh_cfg_from_env();

    let tab = term::open_tab_with_config(&cfg);
    // `term.rs:118` 的 uid 口径：`tab-{稳定 id}`（重连才会追加 `-g{n}`）。
    let uid = format!("tab-{tab}");

    // ① 建连。真机必须已在 known_hosts 中；不在时拒绝提示并让用例失败。
    // 文件存在并不能证明这台主机已被信任，也不能据此自动确认新密钥。
    let t = Instant::now();
    let mut kind = term_diag::term_status_kind(tab);
    while t.elapsed() < Duration::from_secs(60) {
        settle(tab);
        if events::take_prompt().is_some() {
            let _ = events::answer_host_key(&uid, false);
            term::term_close_tab(tab);
            panic!("{target} 的主机密钥未预先受信或已变化；测试已拒绝提示，未修改 known_hosts");
        }
        kind = term_diag::term_status_kind(tab);
        if kind != 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let connect_ms = t.elapsed().as_millis();
    if kind == 5 {
        let note = term_diag::term_status(tab);
        term::term_close_tab(tab);
        panic!("{target} 建连失败（{note:?}，{connect_ms}ms）；检查测试环境后重跑");
    }
    assert_ne!(kind, 1, "60 s 过去仍停在 connecting，既没成功也没报错");

    // ② 提示符上屏。
    let seen_prompt = wait_until(Duration::from_secs(20), || {
        settle(tab);
        let s = term_diag::term_screen_text(tab);
        s.contains('$') || s.contains('#') || s.contains('>')
    });
    assert!(
        seen_prompt,
        "20 s 内没看到 shell 提示符，屏幕内容＝{:?}",
        term_diag::term_screen_text(tab)
    );

    // ③ 命令往返：回显 + 输出，标记各占一次。
    // 计时用 `Cell` 而不是普通 `let mut`：`wait_until` 收的是 `Fn`（非 mut），
    // 在闭包里裸写 `let mut` 捕获报 E0594。
    let marker = format!("TTSH{}", std::process::id());
    // Keep synthetic commands out of the test account's saved POSIX shell history.
    let cmd = format!("unset HISTFILE; printf '%s\\n' {marker}\r");
    let echoed = std::cell::Cell::new(0u128);
    let roundtrip = std::cell::Cell::new(0u128);
    let t = Instant::now();
    term::term_write(tab, cmd.as_bytes());
    // 自己拧 1 ms 的环，不用 `wait_until`（它内部睡 20 ms，会把回显和输出
    // 并进同一次采样——首跑两者都报 2001 ms 就是这个缘故，纯采样假象）。
    let mut ran = false;
    while t.elapsed() < Duration::from_secs(30) {
        settle(tab);
        let s = term_diag::term_screen_text(tab);
        if echoed.get() == 0 && s.contains(marker.as_str()) {
            echoed.set(t.elapsed().as_millis());
        }
        if s.matches(marker.as_str()).count() >= 2 {
            roundtrip.set(t.elapsed().as_millis());
            ran = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let (echoed_ms, roundtrip_ms) = (echoed.get(), roundtrip.get());
    assert!(
        ran,
        "30 s 内没数到两次标记（回显/输出缺一），屏幕＝{:?}",
        term_diag::term_screen_text(tab)
    );
    assert!(term::term_frame(tab).width > 0, "帧缓冲没出画面");

    // 第二个标记：第一次往返如果是"固定 2 s 定时器"造成的，第二次会同样慢；
    // 如果是首包热身（Nagle / 首次窗口 / 首个批），第二次会回到毫秒级。
    let marker2 = format!("TTSH2_{}", std::process::id());
    let cmd2 = format!("printf '%s\\n' {marker2}\r");
    let t2 = Instant::now();
    term::term_write(tab, cmd2.as_bytes());
    let mut second_ms = 0u128;
    let mut ran2 = false;
    while t2.elapsed() < Duration::from_secs(30) {
        settle(tab);
        if term_diag::term_screen_text(tab)
            .matches(marker2.as_str())
            .count()
            >= 2
        {
            second_ms = t2.elapsed().as_millis();
            ran2 = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        ran2,
        "30 s 内没看到第二个标记：{:?}",
        term_diag::term_screen_text(tab)
    );
    // R-21 回归哨兵。默认 200 ms 适用于本地实验网；远程主机按链路情况显式设置阈值。
    let worst = roundtrip_ms.max(second_ms);
    let max_ms = std::env::var("TEST_SSH_MAX_ROUNDTRIP_MS")
        .ok()
        .map(|v| {
            v.parse::<u128>()
                .expect("TEST_SSH_MAX_ROUNDTRIP_MS 必须是毫秒整数")
        })
        .unwrap_or(200);
    assert!(
        worst < max_ms,
        "[R-21] SSH 两次命令往返 {roundtrip_ms}ms / {second_ms}ms，超过本环境阈值 {max_ms}ms"
    );
    println!(
        "[SHELL-SSH] {target}：建连+首帧={connect_ms}ms，命令往返={roundtrip_ms}ms，\
         首次回显={echoed_ms}ms，第二次往返={second_ms}ms（±1ms 采样）"
    );

    // ④ 收尾。
    assert!(term_lifecycle::term_disconnect_tab(tab));
    term::term_close_tab(tab);
}

/// CON-04：**TideTerm 自己的公钥认证路径**（`SshAuth::Key` + `private_key_path`）。
/// 修复前该路径零端到端证据（靶机未放公钥，p2 探针 SKIP）。判据与口令版旅程
/// 同构：建连到已连接 → 提示符上屏 → 标记回显+输出两次 → 断开/关标签不 panic。
/// 凭据：`TEST_SSH_HOST/USER` + `TEST_SSH_KEY`（私钥路径；口令不需要）。
#[test]
#[ignore = "需 TEST_SSH_HOST/USER/KEY；会真连测试机（公钥认证）"]
fn real_ssh_pubkey_login_round_trips_through_the_shell_layer() {
    isolate_known_hosts();
    let _serial = net_serial();
    let _input = input_lock_guard();
    let host = std::env::var("TEST_SSH_HOST").expect("需要 TEST_SSH_HOST");
    let user = std::env::var("TEST_SSH_USER").expect("需要 TEST_SSH_USER");
    let key = std::env::var("TEST_SSH_KEY").expect("需要 TEST_SSH_KEY");
    let port = std::env::var("TEST_SSH_PORT")
        .ok()
        .map(|p| p.parse().expect("TEST_SSH_PORT 必须是有效端口"))
        .unwrap_or(22);
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    {
        let ssh = cfg.ssh.as_mut().expect("blank(Ssh) 自带 SshConfig 默认值");
        ssh.host = host.clone();
        ssh.port = port;
        ssh.username = user;
        ssh.auth = SshAuth::Key;
        ssh.private_key_path = key;
        ssh.connect_timeout_secs = 25;
    }
    let target = format!("{host}:{port}");

    let tab = term::open_tab_with_config(&cfg);
    let uid = format!("tab-{tab}");
    let t = Instant::now();
    let mut kind = term_diag::term_status_kind(tab);
    while t.elapsed() < Duration::from_secs(60) {
        settle(tab);
        if events::take_prompt().is_some() {
            let _ = events::answer_host_key(&uid, false);
            term::term_close_tab(tab);
            panic!("{target} 主机密钥未受信；测试已拒绝提示");
        }
        kind = term_diag::term_status_kind(tab);
        if kind != 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if kind == 5 {
        let note = term_diag::term_status(tab);
        term::term_close_tab(tab);
        panic!("{target} 公钥建连失败（{note:?}）；检查 authorized_keys 与私钥路径");
    }
    assert_ne!(kind, 1, "60 s 过去仍停在 connecting");
    let seen_prompt = wait_until(Duration::from_secs(20), || {
        settle(tab);
        let s = term_diag::term_screen_text(tab);
        s.contains('$') || s.contains('#') || s.contains('>')
    });
    assert!(
        seen_prompt,
        "公钥登录后 20 s 无提示符，屏幕＝{:?}",
        term_diag::term_screen_text(tab)
    );

    let marker = format!("TTKEY{}", std::process::id());
    let cmd = format!("printf '%s\\n' {marker}\r");
    term::term_write(tab, cmd.as_bytes());
    let t = Instant::now();
    let mut ran = false;
    while t.elapsed() < Duration::from_secs(30) {
        settle(tab);
        if term_diag::term_screen_text(tab)
            .matches(marker.as_str())
            .count()
            >= 2
        {
            ran = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        ran,
        "公钥会话 30 s 内标记未往返，屏幕＝{:?}",
        term_diag::term_screen_text(tab)
    );
    println!(
        "[CON-04] {target}：TideTerm 公钥登录 + 命令往返全链路 PASS（建连+首帧={}ms）",
        t.elapsed().as_millis()
    );
    assert!(term_lifecycle::term_disconnect_tab(tab));
    term::term_close_tab(tab);
}

/// SHELL-02/03 的无头代理：zsh / fish 的**转义重输出**经 TideTerm 真实渲染管线
/// 不花屏、往返正常。入口 = 登录 shell（bash）里 `exec zsh` / `exec fish`，
/// 之后各打一个带版本号的标记（回显 + 输出各一次）并断言：
/// ① 版本串上屏（zsh 5.x / fish 3.x）；② 标记两次（shell 换成功了且在跑）；
/// ③ 渲染栅格里**没有裸 ESC 字符**（转义序列漏成可见字符的信号）。
/// 观感项（补全配色/右侧提示符对齐/自动建议灰字）仍需人工——这条只锁协议与格位面。
#[test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS 且靶机装有 zsh 与 fish；会真连测试机"]
fn real_ssh_zsh_and_fish_prompts_render_through_the_shell_layer() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let (cfg, target) = ssh_cfg_from_env();

    let tab = term::open_tab_with_config(&cfg);
    let uid = format!("tab-{tab}");
    let mut kind = term_diag::term_status_kind(tab);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(60) {
        settle(tab);
        if events::take_prompt().is_some() {
            let _ = events::answer_host_key(&uid, false);
            term::term_close_tab(tab);
            panic!("{target} 主机密钥未受信；测试已拒绝提示");
        }
        kind = term_diag::term_status_kind(tab);
        if kind != 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(kind, 1, "60 s 过去仍停在 connecting");
    assert_ne!(kind, 5, "建连失败：{:?}", term_diag::term_status(tab));
    let seen_prompt = wait_until(Duration::from_secs(20), || {
        settle(tab);
        let s = term_diag::term_screen_text(tab);
        s.contains('$') || s.contains('#') || s.contains('>')
    });
    assert!(seen_prompt, "登录 shell 无提示符");

    let root =
        std::env::var("TEST_SSH_FIXTURE_ROOT").expect("private remote fixture root required");
    let suffix = root.strip_prefix("/tmp/tideterm-daily-").unwrap();
    assert!(suffix.len() == 12 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    let zsh = format!(
        "unset HISTFILE; export ZDOTDIR={root}/zsh XDG_CONFIG_HOME={root}/config \
         XDG_DATA_HOME={root}/data XDG_CACHE_HOME={root}/cache fish_history=; \
         export PS1='TT_ZSH_READY> '; exec zsh -d -f"
    );
    for (launch, prompt, probe, marker) in [
        (
            zsh.as_str(),
            "TT_ZSH_READY> ",
            "printf 'TT_ZSH_VERSION=%s\\n' \"$ZSH_VERSION\"",
            "TT_ZSH_VERSION=",
        ),
        (
            "exec fish --no-config -C \"set -g fish_history ''; function fish_prompt; printf 'TT_FISH_READY> '; end\"",
            "TT_FISH_READY> ",
            "printf 'TT_FISH_VERSION=%s\\n' \"$version\"",
            "TT_FISH_VERSION=",
        ),
    ] {
        term::term_write(tab, format!("{launch}\r").as_bytes());
        let seen = wait_until(Duration::from_secs(15), || {
            settle(tab);
            term::with_tab(tab, |st| {
                st.pane.command_prefix().is_some_and(|line| line.ends_with(prompt))
            })
        });
        assert!(seen, "isolated shell did not show its fresh prompt");
        term::term_write(tab, format!("{probe}\r").as_bytes());
        let ok = wait_until(Duration::from_secs(15), || {
            settle(tab);
            term_diag::term_screen_text(tab).lines().any(|line| {
                line.trim().strip_prefix(marker).is_some_and(|version| {
                    version.starts_with(|c: char| c.is_ascii_digit()) && version.contains('.')
                })
            })
        });
        assert!(ok, "isolated shell version output missing");
        // 回显 + 输出各一次 ⇒ 标记至少出现两次（shell 真的换成了且在交互）。
        let n = term_diag::term_screen_text(tab).matches(marker).count();
        assert!(n >= 2, "{launch} 标记只出现 {n} 次（应 ≥2：回显+输出）");
        let s = term_diag::term_screen_text(tab);
        assert!(
            !s.contains('\u{1b}'),
            "{launch} 渲染栅格里出现裸 ESC（转义序列漏成可见字符）：{s:?}"
        );
        println!("[SHELL-SSH {marker}] {target}: fresh prompt/version/echo/render passed");
    }

    assert!(term_lifecycle::term_disconnect_tab(tab));
    term::term_close_tab(tab);
}

/// 真机 SFTP 程序目录：不依赖回环夹具，验证远端目录 → 可接受候选的直接链路。
/// 只读 `/usr/bin` 元数据，不执行远端程序；目录可通过 `TEST_SSH_PROGRAM_DIR` 覆盖。
#[test]
#[ignore = "需 TEST_SSH_HOST/USER/PASS；会真连测试机"]
fn real_ssh_program_directory_scan_returns_sftp_candidates() {
    use crate::bridge::completion::completion_ffi::ProgramSource;
    let _serial = net_serial();
    let _input = input_lock_guard();
    let (cfg, target) = ssh_cfg_from_env();
    let tab = term::open_tab_with_config(&cfg);
    let ready = wait_until(Duration::from_secs(30), || {
        settle(tab);
        super::command_completion::completion_context(tab).ready
    });
    assert!(
        ready,
        "{target} 未在 30 s 内就绪：{:?}",
        term_diag::term_status(tab)
    );
    let context = super::command_completion::completion_context(tab);
    let directory = std::env::var("TEST_SSH_PROGRAM_DIR").unwrap_or_else(|_| "/usr/bin".into());
    let programs = ProgramSource {
        configured: true,
        source: context.source,
        generation: context.generation,
        directories: vec![directory.clone()],
    };
    let owner = super::command_environment::environment_create();
    assert_ne!(owner, 0);
    let mut result =
        super::command_environment::programs_request(owner, &context, "ls", 1, &programs);
    let deadline = Instant::now() + Duration::from_secs(20);
    while result.state == 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        result = super::command_environment::environment_poll(owner);
    }
    let commands: Vec<&str> = result
        .items
        .iter()
        .filter(|item| item.kind == 6)
        .map(|item| item.command.as_str())
        .collect();
    println!(
        "[SHELL-SSH programs] {target} {directory} state={} items={} sample={:?}",
        result.state,
        result.items.len(),
        commands.iter().take(3).collect::<Vec<_>>()
    );
    assert_eq!(
        result.state,
        2,
        "{directory} 远端程序目录扫描为 {}，候选 {} 个",
        result.state,
        result.items.len()
    );
    let expected = format!("{}/ls", directory.trim_end_matches('/'));
    assert!(
        commands.contains(&expected.as_str()),
        "缺少 {expected} 候选：{commands:?}"
    );
    super::command_environment::environment_close(owner);
    assert!(term_lifecycle::term_disconnect_tab(tab));
    term::term_close_tab(tab);
}
