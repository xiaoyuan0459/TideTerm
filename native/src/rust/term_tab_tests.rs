//! 维度 4 的无头补测：多标签的生命周期与 **uid 路由稳定性**。
//!
//! §1 里 TAB-01..06 全是 `gui`，此前一条自动化都没有。这里取其中**不需要人眼**的部分：
//! 标签的计数口径、uid 是否会因为 resize/滚动/IME 等操作漂移（uid 是事件路由键，
//! 也是 C++ 侧文件管理器的绑定键 —— 漂了就等于 SFTP attach 与输出分发挂到错误的会话上）、
//! `protocol_of_uid` 这条 SFTP 域开 channel 的取用路径在开/关之后是否自洽，
//! 以及 IME 提交「走会话 vs 走引擎网格」的**分层边界**。
//!
//! 另附一条 R-11 回归：事件总线积压溢出（`cat` 大文件 / 弱网突发把 16ms 渲染泵
//! 甩开）时，丢帧必须**可观测且可见**，而不是被 `Lagged(_) => continue` 咽掉。
//!
//! 全部走 `term::term_new_tab()`（本地 shell）与 `term_diag::term_feed_inject`，
//! 与机器装了什么 shell 无关，不需要凭据。

use tiderm_core::event::{BackendEvent, BackendEventSink};

use super::term_context;
use super::term_net_tests::net_serial;
use super::term_tests::input_lock_guard;
use super::{events, term, term_diag, term_lifecycle};

// A fresh process prevents another test's first poll from priming the event bus.
#[cfg(windows)]
#[test]
fn startup_output_is_retained_before_first_poll() {
    use std::time::{Duration, Instant};
    const CHILD: &str = "TIDETERM_STARTUP_OUTPUT_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "rust::term_tab_tests::startup_output_is_retained_before_first_poll",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let mut cfg = super::store::session_config::SessionConfig::blank(
        super::store::session_config::SessionKind::Local,
    );
    let local = cfg.local.as_mut().unwrap();
    local.shell = "cmd".into();
    local.args = "/C echo startup-output-retained".into();
    let tab = term::open_tab_with_config(&cfg);
    let protocol = term::protocol_of_uid(&term::term_uid(tab)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while protocol.is_alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!protocol.is_alive(), "finite shell did not exit");
    // Let the output pump finish while UI construction has not yet polled.
    std::thread::sleep(Duration::from_millis(200));
    term::term_poll(tab);
    let output = term_diag::term_screen_text(tab);
    term::term_close_tab(tab);
    assert!(
        output.contains("startup-output-retained"),
        "startup output was lost: {output:?}"
    );
}

// Session identity must be available before the shell emits an OSC title.
#[test]
fn session_titles_use_shell_and_preserve_saved_names() {
    use super::store::session_config::{SessionConfig, SessionKind};
    let _serial = net_serial();
    let _input = input_lock_guard();
    let shells: &[(&str, &str)] = if cfg!(windows) {
        &[
            ("powershell", "PowerShell"),
            ("pwsh", "PowerShell"),
            ("cmd", "Command Prompt"),
        ]
    } else {
        &[("bash", "Bash")]
    };
    for (shell, display) in shells {
        let mut cfg = SessionConfig::blank(SessionKind::Local);
        cfg.local.as_mut().unwrap().shell = (*shell).into();
        let tab = term::open_tab_with_config(&cfg);
        assert_eq!(term_diag::term_title(tab), *display);
        assert_eq!(term_lifecycle::term_session_name(tab), *display);
        assert!(term_diag::registry_snapshot().contains(&(tab, (*display).into())));
        if *display == "PowerShell" {
            term_lifecycle::term_disconnect_tab(tab);
            term::term_poll(tab);
            for alias in [
                "powershell",
                "pwsh.exe",
                "PowerShell.EXE",
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            ] {
                let sequence = format!("\x1b]2;{alias}\x07");
                term_diag::term_feed_inject(tab, sequence.as_bytes());
                term::term_poll(tab);
                assert_eq!(term_diag::term_title(tab), "PowerShell");
            }
        }
        term::term_close_tab(tab);
    }
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    cfg.name = "Build console".into();
    let tab = term::open_tab_with_config(&cfg);
    term_diag::term_feed_inject(tab, b"\x1b]2;Unexpected window title\x07");
    term::term_poll(tab);
    assert_eq!(term_diag::term_title(tab), "Build console");
    term::term_close_tab(tab);
    // Explicit user spelling and arbitrary OSC titles are not executable aliases.
    cfg.name = "powershell custom".into();
    let tab = term::open_tab_with_config(&cfg);
    assert_eq!(term_diag::term_title(tab), "powershell custom");
    term::term_close_tab(tab);
    cfg.name.clear();
    let tab = term::open_tab_with_config(&cfg);
    term_lifecycle::term_disconnect_tab(tab);
    term::term_poll(tab);
    term_diag::term_feed_inject(tab, b"\x1b]2;build MixedCASE\x07");
    term::term_poll(tab);
    assert_eq!(term_diag::term_title(tab), "build MixedCASE");
    term::term_close_tab(tab);
}

// Contract: cwd notifications belong to their terminal; splits inherit the focused
// session's cwd, while a remote file URI cannot point the local browser at a share.
#[test]
fn cwd_is_routed_by_uid_and_inherited_on_split() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let dir = tempfile::tempdir().unwrap();
    let first = term::term_new_tab();
    let second = term::term_new_tab();
    let original = term_context::term_current_dir(second);
    let cwd = dir.path().to_string_lossy().into_owned();
    let sequence = format!("\x1b]9;9;{cwd}\x07");
    events::sink().emit(BackendEvent::Output {
        id: term::term_uid(first),
        data: sequence.into_bytes(),
    });
    term::term_poll(first);
    assert_eq!(term_context::term_current_dir(first), cwd);
    assert_eq!(term_context::term_current_dir(second), original);
    let duplicate = term_context::term_split_tab(first);
    assert_eq!(term_context::term_current_dir(duplicate), cwd);
    term_diag::term_feed_inject(first, b"\x1b]7;file://untrusted-host/share\x07");
    assert_eq!(term_context::term_current_dir(first), cwd);
    for id in [duplicate, second, first] {
        term::term_close_tab(id);
    }
}

/// TAB-01b：标签计数与 uid 唯一性 —— 「开 N 个、关 1 个、数剩下几个」这层口径要对，
/// 否则并发场景下"标签数 - 实际会话数"的差值会一路滚大。
#[test]
fn tab_count_tracks_open_and_close_without_drift() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let base = term::term_tab_count();
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(term::term_new_tab());
        term_lifecycle::reserve_tab_ids(1);
    }
    assert_eq!(term::term_tab_count(), base + 4, "开 4 个标签后计数不对");
    let uids: Vec<String> = ids.iter().map(|&t| term::term_uid(t)).collect();
    for (i, u) in uids.iter().enumerate() {
        assert!(!u.is_empty(), "标签 {} 的 uid 是空的", ids[i]);
        assert!(
            !uids[..i].contains(u),
            "标签 {} 与更早的标签撞了同一个 uid {u} —— 事件会串台",
            ids[i]
        );
    }
    term::term_close_tab(ids[2]);
    assert_eq!(term::term_tab_count(), base + 3, "关掉 1 个后计数不对");
    // 关中间那个，兄弟标签必须毫发无损。
    assert!(!term::term_uid(ids[0]).is_empty(), "兄弟标签被连带清掉了");
    for t in [ids[0], ids[1], ids[3]] {
        term::term_close_tab(t);
    }
    assert_eq!(term::term_tab_count(), base, "收尾没回到基线");
}

/// uid 是**事件路由键**（`BackendEvent.id`）与 **C++ 文件管理器绑定键**。
/// resize / 滚动 / IME 这些高频操作一律不许让它漂移 —— 漂了的表现是
/// 「标签还在，但输出发不进来 / SFTP 面板挂到别的会话上」，且**没有任何报错**。
#[test]
fn session_uid_survives_resize_scroll_and_ime() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_lifecycle::reserve_tab_ids(1);
    let uid = term::term_uid(tab);
    assert!(uid.starts_with("tab-"), "uid 口径变了：{uid}");

    // resize：网格尺寸会变，uid 不能跟着变。
    let before = term_diag::term_stats(tab);
    assert!(term::term_resize_px(tab, 600.0, 400.0), "resize 失败");
    assert_ne!(
        term_diag::term_stats(tab).cols,
        before.cols,
        "缩窗没生效（本测例的断言就没有意义了）"
    );
    assert_eq!(term::term_uid(tab), uid, "resize 把 uid 弄漂了");

    // 滚动 + IME 提交：都不该碰路由键。
    term::term_scroll_edge(tab, true);
    term::term_scroll_edge(tab, false);
    assert_eq!(term::term_uid(tab), uid, "滚动把 uid 弄漂了");
    term::term_ime_commit(tab, "字");
    assert_eq!(term::term_uid(tab), uid, "IME 提交把 uid 弄漂了");

    // 关掉之后 uid 解析必须失效（否则 C++ 侧拿着旧键会操作到已销毁的会话）。
    term::term_close_tab(tab);
    assert!(
        term::protocol_of_uid(&uid).is_none(),
        "标签关了之后 uid 还能解析出协议 —— 悬垂路由键"
    );
}

/// `protocol_of_uid` 是 SFTP 域开 channel 的唯一入口（`SshInstance::open_sftp_channel`）。
/// 本地 shell 会话应当**当场拿得到**句柄；拿不到就意味着 SFTP 面板对本地会话全不可用。
#[test]
fn protocol_of_uid_resolves_a_live_local_session() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_lifecycle::reserve_tab_ids(1);
    let uid = term::term_uid(tab);
    assert!(
        term::protocol_of_uid(&uid).is_some(),
        "本地会话开完之后 protocol_of_uid 取不到句柄 —— SFTP 域无从开 channel"
    );
    term::term_close_tab(tab);
    assert!(term::protocol_of_uid(&uid).is_none(), "关闭后仍可解析");
    // 不存在的 uid 要安静返回 None，不能 panic。
    assert!(term::protocol_of_uid("tab-does-not-exist").is_none());
}

/// IME 提交路径的**分层边界**（中英文混排的输入侧）：`term_ime_commit` 走**会话写路径**
/// （`term_write` → `host.write` → PTY），**不直接画进引擎网格** —— 中文要等 shell 回显、
/// 经读循环、泵回网格才上屏。这条把边界钉住：谁哪天让 IME 直接喂引擎（"更省事"），
/// 本测例会红，因为那会让**远端**会话看不到输入（本地 PTY 有回显，反而看不出问题）。
#[test]
fn ime_commit_goes_to_the_session_not_the_engine_grid() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_lifecycle::reserve_tab_ids(1);
    let before = term_diag::term_stats(tab);

    // ① IME 提交不得直接动网格：光标与视口文本都应保持原样。
    term::term_ime_commit(tab, "中文");
    let after = term_diag::term_stats(tab);
    assert_eq!(
        (after.cursor_col, after.cursor_row),
        (before.cursor_col, before.cursor_row),
        "IME 提交推动了光标 —— 它直接画进了网格而不是走会话"
    );
    assert!(
        term_diag::term_screen_text(tab).is_empty(),
        "IME 提交直接画出了文本：{:?}",
        term_diag::term_screen_text(tab)
    );

    // ② 空提交是彻底的 no-op（不往 pty 写空包）。
    term::term_ime_commit(tab, "");
    let again = term_diag::term_stats(tab);
    assert_eq!(
        (again.cursor_col, again.cursor_row),
        (before.cursor_col, before.cursor_row),
        "空提交也动了状态"
    );

    // ③ 对照：同一串字走 `feed_inject`（引擎路径）必须立刻渲染 —— 证明上面两条
    //    不是渲染坏了，而是两条路径本来就该不同。
    term_diag::term_feed_inject(tab, "中文".as_bytes());
    assert_eq!(
        term_diag::term_screen_text(tab),
        "中文",
        "引擎路径反而画不出来"
    );
    term::term_close_tab(tab);
}

/// R-11 回归：事件总线积压溢出时，`drain_events` 原来把 `Lagged(_)` 一句 `continue`
/// 咽掉 —— 被丢的 `Output` 是**永久丢失**（读循环不重发），屏幕就此缺行；被丢的 `Exit`
/// 更糟：会话已经死了，标签上却不显示「已结束」。这两种都**不报错**，与 R-16 同类。
/// 修复口径：丢帧数必须可观测，且**所有**会话都要重绘 + 拿到一条可见提示（`Lagged(n)`
/// 只给条数、无法归属到 uid，只能保守全量）。
#[test]
fn lagged_event_stream_resyncs_every_pane_and_says_so() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let victim = term::term_new_tab();
    term_lifecycle::reserve_tab_ids(1);
    let bystander = term::term_new_tab();
    term_lifecycle::reserve_tab_ids(1);
    // 先把已有积压抽干（本地 shell 有没有回执是时序问题，返回值不作断言）。
    term::term_poll(victim);
    term::term_poll(bystander);

    let before = events::dropped_events();
    // 灌到溢出（容量取自 core 的真常量，不抄魔数）且**中途不 poll** —— 这就是
    // `cat` 大文件 / 弱网突发时 16ms 渲染泵被读写循环甩开的形态。
    let flood = BackendEventSink::EVENT_CHANNEL_CAPACITY + 64;
    for i in 0..flood {
        events::sink().emit(BackendEvent::Output {
            // uid 取无人认领的串：这些事件的去向是"被丢弃"，不该污染任何会话。
            id: format!("r11-flood-{i}"),
            data: b"x".to_vec(),
        });
    }

    // 第一个 poll 把积压抽干，也正是在这次 drain 里才可能撞见 `Lagged(n)`。
    assert!(term::term_poll(victim), "丢弃后第一个会话没有重绘请求");
    assert!(
        events::dropped_events() > before,
        "溢出了却没记到丢弃数 —— 又是静默丢弃"
    );
    // 第二个 poll 时事件流已空，它还能拿到重绘请求，说明标记确实覆盖了
    // 所有会话 —— 谁丢的不知道，只能全量。
    assert!(
        term::term_poll(bystander),
        "丢弃后旁观会话没被标记重绘 —— 丢弃被静默了"
    );
    assert!(
        term_diag::term_status(bystander).contains("输出丢帧"),
        "缺行没有可见提示（状态仍是 {:?}）",
        term_diag::term_status(bystander)
    );
    term::term_close_tab(victim);
    term::term_close_tab(bystander);
}
