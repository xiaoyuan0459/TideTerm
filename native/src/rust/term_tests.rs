use tiderm_core::event::BackendEvent;

use super::{events, term, term_lifecycle, term_log, term_ops, term_selection};

#[test]
fn session_log_panel_reads_active_output_and_clears_it() {
    let tab = term::term_new_tab();
    term_log::slog_start(tab);
    let before = term_log::slog_revision(tab);
    term::with_tab(tab, |st| st.log.append(b"TIDETERM_LOG_PANEL_MARKER\r\n"));
    assert!(term_log::slog_revision(tab) > before);
    assert!(term_log::slog_recent(tab, 100).contains("TIDETERM_LOG_PANEL_MARKER"));
    term_log::slog_clear(tab);
    assert!(term_log::slog_recent(tab, 100).is_empty());
    term::term_close_tab(tab);
}

/// `term_ops::term_set_locked` 是**全局**状态：凡对键盘输入敏感的测试须串行，
/// 否则锁窗会随机吞掉其它测试的 `term_key`（实测会让「裸 PgUp 仍发 CSI」偶判空）。
/// `pub(super)` 给 `term_esc_tests.rs` 复用——static 在函数内，两个模块拿到的是同一把锁。
pub(super) fn input_lock_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn lock_rejects_keyboard_input_until_unlocked() {
    let _serial = input_lock_guard();
    let tab = term::term_new_tab();
    term_ops::term_set_locked(true);
    assert!(term::term_key(tab, 0, 0, "x").is_empty());
    assert!(!term::term_paste_from_clipboard(tab));
    term_ops::term_set_locked(false);
    assert_eq!(term::term_key(tab, 0, 0, "x"), b"x");
    term::term_close_tab(tab);
}

#[test]
fn output_for_two_panes_keeps_both_repaint_requests() {
    // 事件总线是进程级全局：不串行的话，并发测试的洪水会把这里的 Output 挤出
    // 4096 积压，令 `assert!(term_poll(..))` 假红。与键盘锁共用一把串行锁。
    let _serial = input_lock_guard();
    let first = term::term_new_tab();
    let second = term::term_new_tab();
    term::term_poll(first);
    term::term_poll(second);

    for id in [first, second] {
        events::sink().emit(BackendEvent::Output {
            id: format!("tab-{id}"),
            data: format!("pane-{id}\r\n").into_bytes(),
        });
    }

    term::term_pump();
    assert!(
        term::term_take_redraw(second),
        "后领取的窗格不能丢失输出通知"
    );
    assert!(term::term_take_redraw(first), "其他窗格也应保留重绘通知");
    assert!(!term::term_take_redraw(first), "同一请求只能领取一次");
    assert!(!term::term_take_redraw(second), "同一请求只能领取一次");
    for id in [first, second] {
        assert!(super::term_diag::term_screen_text(id).contains(&format!("pane-{id}")));
    }
    // A benchmark feed must request the same one-shot repaint as transport output.
    // Disconnect first so no background shell notification can satisfy this check.
    assert!(term_lifecycle::term_disconnect_tab(first));
    assert!(term_lifecycle::term_disconnect_tab(second));
    term::term_pump();
    for id in [first, second] {
        term::term_take_redraw(id);
        super::term_diag::term_feed_inject(id, b"INJECTED_REDRAW\r\n");
        super::term_diag::term_feed_inject(id, b"COALESCED\r\n");
    }
    assert!(
        term::term_take_redraw(second),
        "injected output needs repaint"
    );
    assert!(
        term::term_take_redraw(first),
        "other pane keeps its repaint"
    );
    assert!(!term::term_take_redraw(first));
    assert!(!term::term_take_redraw(second));
    super::term_diag::term_feed_inject(first, b"");
    assert!(
        !term::term_take_redraw(first),
        "empty feed must not wake painting"
    );
    term::term_close_tab(second);
    term::term_close_tab(first);
}

/// Measure real pane scheduling separately from the engine's feed/render G1
/// benchmark. Keep the shell children and paint work outside the timed loop.
#[test]
#[ignore = "manual Release polling benchmark: warmup plus three samples"]
fn polling_eight_panes_release_benchmark() {
    if cfg!(debug_assertions) {
        panic!("run with --release");
    }
    let _serial = input_lock_guard();
    let before = term::term_tab_count();
    let ids: Vec<_> = (0..8).map(|_| term::term_new_tab()).collect();
    std::thread::sleep(std::time::Duration::from_secs(2));
    for sample in 0..4 {
        for id in &ids {
            events::sink().emit(BackendEvent::Output {
                id: term::term_uid(*id),
                data: format!("POLL_BENCH_{sample}\r\n").into_bytes(),
            });
        }
        let start = std::time::Instant::now();
        for _ in 0..20_000 {
            term::term_pump();
            for id in &ids {
                std::hint::black_box(term::term_take_redraw(*id));
            }
        }
        let per_tick_us = start.elapsed().as_secs_f64() * 1e6 / 20_000.0;
        for id in &ids {
            assert!(
                super::term_diag::term_screen_text(*id).contains(&format!("POLL_BENCH_{sample}"))
            );
        }
        println!(
            "[poll-bench] mode=batch panes=8 sample={sample} warmup={} tick_us={per_tick_us:.3}",
            sample == 0
        );
    }
    for id in ids {
        term::term_close_tab(id);
        assert!(
            !term::term_poll(id),
            "closed pane remained eligible for repaint"
        );
    }
    assert_eq!(term::term_tab_count(), before);
}

#[test]
fn shift_scroll_keys_roll_viewport_instead_of_reaching_pty() {
    let _serial = input_lock_guard();
    // 对齐批次 11：引擎把 Shift+PgUp/PgDn、Shift+Home/End 定义为**回滚手势**
    // （keys.rs:252-255 明写「the app shell calls this first」），壳必须自己先判。
    let tab = term::term_new_tab();
    term::term_poll(tab);
    for i in 0..300 {
        events::sink().emit(BackendEvent::Output {
            id: format!("tab-{tab}"),
            data: format!(
                "line {i}
"
            )
            .into_bytes(),
        });
    }
    assert!(term::term_poll(tab), "300 行输出应触发重绘");
    assert_eq!(
        crate::rust::term_diag::term_scroll_offset(tab),
        0,
        "初始贴底"
    );

    assert!(
        term::term_key(tab, 7, 1, "").is_empty(),
        "Shift+PgUp 不得编码成 pty 输入"
    );
    let mid = crate::rust::term_diag::term_scroll_offset(tab);
    assert!(mid > 0, "Shift+PgUp 应滚出历史（实测 offset={mid}）");

    term::term_key(tab, 5, 1, "");
    assert!(
        crate::rust::term_diag::term_scroll_offset(tab) >= mid,
        "Shift+Home 跳到历史顶部"
    );
    term::term_key(tab, 6, 1, "");
    assert_eq!(
        crate::rust::term_diag::term_scroll_offset(tab),
        0,
        "Shift+End 回到底"
    );

    assert!(
        !term::term_key(tab, 7, 0, "").is_empty(),
        "裸 PgUp 仍须发 CSI"
    );
    term::term_close_tab(tab);
}

#[test]
fn reconnect_rejects_output_from_previous_transport() {
    let _serial = input_lock_guard();
    let tab = term::term_new_tab();
    let old_uid = term::term_uid(tab);
    assert!(term_lifecycle::term_disconnect_tab(tab));
    assert!(term_lifecycle::term_reconnect_tab(tab));
    let new_uid = term::term_uid(tab);
    assert_ne!(old_uid, new_uid);
    term::term_poll(tab);
    events::sink().emit(BackendEvent::Output {
        id: old_uid,
        data: b"OLD_TRANSPORT_MARKER\r\n".to_vec(),
    });
    events::sink().emit(BackendEvent::Output {
        id: new_uid,
        data: b"NEW_TRANSPORT_MARKER\r\n".to_vec(),
    });
    assert!(term::term_poll(tab));
    let screen = super::term_diag::term_screen_text(tab);
    assert!(screen.contains("NEW_TRANSPORT_MARKER"));
    assert!(!screen.contains("OLD_TRANSPORT_MARKER"));
    term::term_close_tab(tab);
}

#[test]
fn menu_search_and_selection_use_terminal_engine() {
    let tab = term::term_new_tab();
    super::term_diag::term_feed_inject(tab, b"TIDETERM_SEARCH_MARKER\r\n");
    let result = term_ops::term_search_set(tab, "TIDETERM_SEARCH_MARKER", 0);
    assert!(result.ends_with("/1"), "{result}");
    assert!(term_ops::term_search_active_rect(tab).w > 0.0);
    term_ops::term_select_all(tab);
    assert!(term_ops::term_selected_text(tab).contains("TIDETERM_SEARCH_MARKER"));
    term_ops::term_deselect(tab);
    assert!(term_ops::term_selected_text(tab).is_empty());
    term::term_close_tab(tab);
}

#[test]
fn select_to_and_swap_anchor_report_failure_without_a_selection() {
    let tab = term::term_new_tab();
    super::term_diag::term_feed_inject(tab, b"0123456789\r\nABCDEFGHIJ\r\n");
    // No selection yet: both entry points must decline so the view can grey the menu out and
    // fall back to a fresh drag, rather than leaving a half-built selection behind.
    assert!(!term_selection::term_selection_active(tab));
    assert!(!term_selection::term_selection_extend_to(
        tab, 25.0, 10.0, false
    ));
    assert!(!term_selection::term_selection_swap_anchor(tab));

    term_ops::term_select_all(tab);
    assert!(term_selection::term_selection_active(tab));
    let before = term_ops::term_selected_text(tab);
    assert!(term_selection::term_selection_swap_anchor(tab));
    assert_eq!(
        term_ops::term_selected_text(tab),
        before,
        "swapping the ends must not change the copied text"
    );
    assert!(term_selection::term_selection_extend_to(
        tab, 25.0, 10.0, false
    ));

    term_ops::term_deselect(tab);
    assert!(!term_selection::term_selection_active(tab));
    assert!(!term_selection::term_selection_extend_to(
        tab, 25.0, 10.0, false
    ));
    term::term_close_tab(tab);
}

#[test]
fn select_to_on_a_closed_tab_is_inert() {
    let tab = term::term_new_tab();
    term::term_close_tab(tab);
    assert!(!term_selection::term_selection_active(tab));
    assert!(!term_selection::term_selection_extend_to(
        tab, 25.0, 10.0, false
    ));
    assert!(!term_selection::term_selection_swap_anchor(tab));
    assert_eq!(term_selection::term_selection_status(tab).count, 0);
    assert!(!term_selection::term_set_multi_selection(tab, true));
    assert!(!term_selection::term_selection_begin_add(
        tab, 25.0, 10.0, false
    ));
    assert!(!term_selection::term_selection_cycle(tab, true));
    assert!(!term_selection::term_selection_remove_active(tab));
    assert!(term_selection::term_selection_active_rects(tab).is_empty());
}

#[test]
fn font_zoom_resizes_existing_terminal_and_restores_default() {
    let tab = term::term_new_tab();
    let old = term_ops::term_font_px();
    let before = super::term_diag::term_stats(tab);
    assert!(term_ops::term_set_font_px(old + 4));
    let after = super::term_diag::term_stats(tab);
    assert_eq!(term_ops::term_font_px(), old + 4);
    assert!(after.cell_h > before.cell_h);
    let surface = term::term_frame(tab);
    assert!(surface.width > 0 && surface.height > 0);
    assert!(term_ops::term_set_font_px(old));
    term::term_close_tab(tab);
}

/// 对齐批次 A：终端正文配色随主题（旧实现写死单一深色，换主题终端区不变）。
#[test]
fn terminal_palette_follows_theme_and_reaches_live_panes() {
    // 引擎约定（grid::set_base16）：base16[0]=默认背景、[7]=默认前景。
    let windterm = term_ops::theme_base16_for("windterm");
    assert_eq!(windterm[0], [0x1c, 0x1c, 0x1c]);
    assert_eq!(windterm[7], [0xbb, 0xbb, 0xbb]);
    // 换主题 ⇒ 换调色板。
    assert_ne!(windterm, term_ops::theme_base16_for("light"));
    // light 主题刻意区分 shell 底（#f5f5f5）与终端底（#ffffff）——
    // PtyView 空白区必须跟终端走 base16[0]，否则与引擎渲出的单元格接缝色差。
    assert_eq!(term_ops::theme_base16_for("light")[0], [0xff, 0xff, 0xff]);
    let tab = term::term_new_tab();
    term_ops::term_apply_palette();
    term::term_close_tab(tab);
}
