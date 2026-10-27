//! native —— TideTerm Qt/C++ 壳（ADR-012 ✅ Accepted；Phase B 骨架）。
//!
//! 模块：`ffi`（cxx 桥声明，只传 POD）→ `rust::term`（引擎宿主）/ `rust::session`
//! （PTY 会话宿主）→ `selftest`（G1/G2 闸门套件）。C++ 侧在 `cpp/`（PtyView +
//! 编排器 + G3 SFTP 试点）。
//!
//! 模式：--selftest / --pty-selftest / --g1-selftest / --g1-bench / --g2-selftest /
//! --g3-selftest / --pty（交互）。

use std::env;
use std::sync::{atomic::Ordering, OnceLock};

// 大块分配取证（§8.6.3）：默认零行为，`TIDETERM_TRACE_BIG_ALLOC=<字节>` 才打印调用栈。
#[global_allocator]
static ALLOC_PROBE: rust::alloc_probe::BigAllocProbe = rust::alloc_probe::BigAllocProbe;

// cxx 桥声明（POD：RawSurface/Rect/TermStats，只传 POD）。
// 挂载规则（ADR-014 §Decision 3，spike 已实证）：桥内联本文件 **或** 按域拆到
// src/bridge/（文件模块 + 桥文件顶层 pub use 重导出实现符号，供生成代码 super::
// 解析——缺重导出才 E0425 全量挂）。主桥暂留此处，新域（~35 函数扩容）走 bridge/。
// extern "Rust" 声明在 C++ 全局作用域：跨桥函数名必须全局唯一。
#[cxx::bridge]
mod ffi {
    /// 跨桥帧 POD（ADR-013 RawSurface 的 cxx 孪生；ptr 过桥转 usize —— cxx 不收裸指针）。
    pub struct RawSurface {
        pub ptr: usize,
        pub width: u32,
        pub height: u32,
        pub stride: u32,
    }

    /// 光标像素矩形（G2 IME 锚定：ImCursorRectangle = 引擎光标格 × 字体度量）。
    pub struct Rect {
        pub x: f32,
        pub y: f32,
        pub w: f32,
        pub h: f32,
    }

    /// 引擎统计（自检断言 / 状态栏）。
    pub struct TermStats {
        pub cols: u32,
        pub rows: u32,
        pub cursor_col: u32,
        pub cursor_row: u32,
        /// 行号栏宽度（文本格数，0 = 无栏）。`cols` 不含它，光标与内容都在它右边。
        pub gutter_cells: u32,
        pub surface_w: u32,
        pub surface_h: u32,
        pub cell_w: u32,
        pub cell_h: u32,
    }

    extern "Rust" {
        // ---- 脚手架假引擎（--selftest 保留） ----
        fn frame_for(frame_index: u32) -> RawSurface;
        fn on_frame_painted(painted: u32);
        fn on_key(key: i32) -> bool;

        // ---- 真实引擎（term.rs；Phase C ①：全部按标签索引，状态在 Rust 侧） ----
        /// 新建标签（独立 PTY 会话 + 引擎），返回索引。
        fn term_new_tab() -> u32;
        /// 关闭标签（drop 会话宿主 → PTY 终止）。返回剩余数。
        fn term_close_tab(idx: u32) -> u32;
        fn term_disconnect_tab(idx: u32) -> bool;
        fn term_reconnect_tab(idx: u32) -> bool;
        fn term_session_name(idx: u32) -> String;
        fn term_session_url(idx: u32) -> String;
        fn term_select_all(idx: u32);
        fn term_selected_text(idx: u32) -> String;
        fn term_set_locked(locked: bool);
        fn term_write_sessions(ids: &[u32], bytes: &[u8], raw: bool) -> u32;
        /// 主题 base16 下发全部窗格（终端正文配色随主题）。
        fn term_apply_palette();
        /// 当前主题 base16 的 48 字节平面视图（自检：chrome 取色与引擎调色板同源）。
        fn term_palette_probe() -> Vec<u8>;
        fn term_font_px() -> u32;
        fn term_set_font_px(px: u32) -> bool;
        /// 设置设备像素比（HiDPI）：引擎按物理分辨率光栅化，桥面坐标仍为逻辑像素。
        fn term_set_scale(scale: f32) -> bool;
        fn term_deselect(idx: u32);
        fn term_selection_rects(idx: u32) -> Vec<Rect>;
        fn term_search_set(idx: u32, query: &str, flags: u32) -> String;
        fn term_search_query(idx: u32) -> String;
        fn term_search_flags(idx: u32) -> u32;
        fn term_clear(idx: u32, kind: u8);
        fn term_cursor_tick(idx: u32, focused: bool);
        fn term_link_target(idx: u32, x: f32, y: f32) -> String;
        fn term_search_step(idx: u32, forward: bool) -> String;
        fn term_search_rects(idx: u32) -> Vec<Rect>;
        fn term_search_active_rect(idx: u32) -> Rect;
        fn term_tab_count() -> u32;
        /// 标签标题：保存名称优先，随后 OSC 标题或实际 Shell/连接目标。
        fn term_title(idx: u32) -> String;
        /// 会话 uid（"tab-{id}"——SFTP attach/事件路由键）。
        fn term_uid(idx: u32) -> String;
        /// 会话泵：drain PTY 事件 → 喂 pane。true = 有新输出。
        fn term_poll(idx: u32) -> bool;
        fn term_pump();
        fn term_take_redraw(idx: u32) -> bool;
        /// 渲染（脏了才画）并返回帧。有效期：至下一次调用（QImage 每次 paint 现构造）。
        fn term_frame(idx: u32) -> RawSurface;
        /// 生产绘制面：**无像素**的绘制记录流（每 6 个 u32 一条，线格式见 vendor
        /// `tiderm-terminal/src/spans.rs`）。字形由 Qt 文本栈光栅化。
        fn term_spans(idx: u32) -> Vec<u32>;
        /// 终端字体族名（与 Rust 侧度量来源同一张脸；QFont 用名字）。
        fn term_font_family() -> String;
        /// 内存探针：引擎侧常驻分量的一行报告（网格/历史/回滚/帧缓冲/字形缓存）。
        fn term_mem_report() -> String;
        /// 视口尺寸 → 网格重算 + 会话 resize。
        fn term_resize_px(idx: u32, w: f32, h: f32) -> bool;
        /// 键盘：key=中立键码（0=Char 走 text；1..15 功能键；32+n=F(n+1)）；
        /// mods bit0=shift bit1=alt bit2=ctrl。返回写入 PTY 的字节。
        fn term_key(idx: u32, key: u32, mods: u32, text: &str) -> Vec<u8>;
        /// 直接写 PTY（粘贴 / 自检喂命令）。
        fn term_write(idx: u32, bytes: &[u8]);
        /// 滚轮（正 = 向上回滚）。返回鼠标上报字节。
        fn term_wheel(idx: u32, lines: i32, x: f32, y: f32) -> Vec<u8>;
        /// 应用开启 mouse-tracking 时的上报序列。
        fn term_mouse(idx: u32, kind: u8, button: u8, mods: u32, x: f32, y: f32) -> Vec<u8>;
        fn term_mouse_send(idx: u32, kind: u8, button: u8, x: f32, y: f32) -> bool;
        // 选区（G1-4）：拖拽语义 end 自动复制，与 Slint 壳同。
        fn term_selection_begin(idx: u32, x: f32, y: f32);
        fn term_selection_update(idx: u32, x: f32, y: f32);
        fn term_selection_end(idx: u32) -> u32;
        fn term_paste_from_clipboard(idx: u32) -> bool;
        // 滚动手势（G1-5）：Shift+PageUp/Down 与 Shift+Home/End。
        fn term_scroll_page(idx: u32, up: bool);
        fn term_scroll_edge(idx: u32, top: bool);
        /// 视口回滚偏移（行数）——自检探针。
        fn term_scroll_offset(idx: u32) -> u32;
        // 渲染选项（G1-3/G1-6）：光标三态 + gutter。
        fn term_set_render_opts(idx: u32, cursor_style: u8, gutter: bool);
        // G2 IME：组合窗锚位 + 提交串写 PTY。
        fn term_caret_rect(idx: u32) -> Rect;
        fn term_ime_commit(idx: u32, text: &str);
        // 自检/基准辅助。
        fn term_feed_inject(idx: u32, bytes: &[u8]);
        fn term_screen_text(idx: u32) -> String;
        fn term_stats(idx: u32) -> TermStats;
        fn term_feed_render_bench(bursts: u32, lines_per: u32) -> f64;
        fn term_status(idx: u32) -> String;
        /// 连接状态类型码（tab 角标/树状态点取色；见 rust/term_diag.rs）。
        fn term_status_kind(idx: u32) -> u8;
        fn term_renderer_name(idx: u32) -> String;
        fn term_pty_selftest_assert() -> bool;
        fn term_g2_selftest_assert() -> bool;
    }

    unsafe extern "C++" {
        include!("cpp/bridge_shim.h");
        fn qt_version() -> String;
        fn qt_has_gui_application() -> bool;
        fn qt_clipboard_copy(text: &str) -> bool;
        fn qt_clipboard_read() -> String;
        fn qt_clipboard_has_image() -> bool;
        /// 脚手架自检（假引擎 N 帧，离屏落盘）。
        fn qt_boot(selftest: bool, frames: u32, png_out: &str) -> i32;
        fn qt_memory_probe(stage: u8) -> i32;
        fn qt_boot_welcome(png_out: &str) -> i32;
        /// 真实引擎自检：本地 PTY + 喂命令 + 泵 + 渲染 + 离屏 PNG + 断言。
        fn qt_boot_pty(feed_cmd: &str, png_out: &str) -> i32;
        /// G2 IME 自检：合成输入法事件走真实 Qt 分发路径。
        fn qt_boot_g2(png_out: &str) -> i32;
        /// G3 系数试点：Qt 版 SFTP 浏览器离屏构建 + 结构断言 + PNG。
        fn qt_boot_g3(png_out: &str) -> i32;
        /// Phase C ①：多标签窗口自检（隔离/生命周期/标题）。
        fn qt_boot_c1(png_out: &str) -> i32;
        /// Phase C ②：分割/关窗格自检。
        fn qt_boot_c2(png_out: &str) -> i32;
        /// Phase C ③：chrome 骨架自检（菜单/横幅/发送栏/dock/树模型）。
        fn qt_boot_c3(png_out: &str) -> i32;
        /// Phase C ④：会话树+表单+快速连接自检。
        fn qt_boot_c4(png_out: &str) -> i32;
        /// Phase C ⑤：SFTP+传输自检。
        fn qt_boot_c5(png_out: &str) -> i32;
        /// Phase C ⑥：AI 面板自检。
        fn qt_boot_c6(png_out: &str) -> i32;
        /// Phase C ⑦：偏好+主题自检。
        fn qt_boot_c7(png_out: &str) -> i32;
        /// Phase C ⑧：状态栏/日志自检。
        fn qt_boot_c8(png_out: &str) -> i32;
        /// 对齐批次 A/B：webview 度量自检（五区高度/dock 钳位/QSS 令牌/取色）。
        fn qt_boot_c9(png_out: &str) -> i32;
        /// i18n 自检：翻译表加载/命中/回落三态（--i18n-selftest）。
        fn qt_i18n_selftest() -> i32;
        /// Phase C ①：交互主窗口（窗口+标签+单终端）。
        fn qt_window(profile: &str) -> i32;
        /// 交互模式（阻塞到窗口关闭）。
        fn qt_run_pty() -> i32;
    }
}

mod bridge;
mod rust;
mod selftest;

// cxx 的 extern "Rust" 在 crate 根解析符号：实现在 rust::term / selftest，
// 此处逐名根导出（桥内联于上方，符号必须可达）。
pub use rust::term::{
    term_caret_rect, term_close_tab, term_frame, term_ime_commit, term_key, term_mouse,
    term_mouse_send, term_new_tab, term_paste_from_clipboard, term_poll, term_pump, term_resize_px,
    term_scroll_edge, term_scroll_page, term_selection_begin, term_selection_end,
    term_selection_update, term_set_render_opts, term_tab_count, term_take_redraw, term_uid,
    term_wheel, term_write,
};
pub use rust::term_actions::{term_clear, term_cursor_tick, term_link_target};
pub use rust::term_diag::{
    term_cpu_bytes, term_feed_inject, term_feed_render_bench, term_mem_report, term_renderer_name,
    term_screen_text, term_scroll_offset, term_stats, term_status, term_status_kind, term_title,
};
pub use rust::term_lifecycle::{
    term_disconnect_tab, term_reconnect_tab, term_session_name, term_session_url,
};
pub use rust::term_ops::{
    term_apply_palette, term_deselect, term_font_family, term_font_px, term_palette_probe,
    term_search_active_rect, term_search_flags, term_search_query, term_search_rects,
    term_search_set, term_search_step, term_select_all, term_selected_text, term_selection_rects,
    term_set_font_px, term_set_locked, term_set_scale, term_spans, term_write_sessions,
};
pub use selftest::{g1_bench, g1_selftest, term_g2_selftest_assert, term_pty_selftest_assert};
// Phase C ④：session_config/session_store 挂到 crate 根可达位
//（session_store.rs 以 `crate::session_config` 引用其数据模型）。
pub use rust::store::{session_config, session_store};
// Phase C ⑦：主题服务挂 crate 根——theme_db/ui_prefs 内部以
// `crate::theme::…` 引用 ThemeSpec/DEFAULT_THEME/by_id；crate::rust::theme 由
// rust/mod.rs pub use 同一模块（避免双挂载两份类型）。
#[path = "rust/theme.rs"]
pub mod theme;

// ════════════════════ 脚手架假引擎（--selftest 保留） ════════════════════

const SURFACE_W: usize = 640;
const SURFACE_H: usize = 400;
const BYTES_PER_PIXEL: usize = 4;

fn surface_ptr() -> usize {
    static PTR: OnceLock<usize> = OnceLock::new();
    *PTR.get_or_init(|| {
        let buf = vec![0u8; SURFACE_W * SURFACE_H * BYTES_PER_PIXEL];
        Box::leak(buf.into_boxed_slice()).as_mut_ptr() as usize
    })
}

pub fn frame_for(frame_index: u32) -> ffi::RawSurface {
    let ptr = surface_ptr();
    let len = SURFACE_W * SURFACE_H * BYTES_PER_PIXEL;
    unsafe {
        let buf = std::slice::from_raw_parts_mut(ptr as *mut u8, len);
        let bar_x = (frame_index as usize * 67) % SURFACE_W;
        for y in 0..SURFACE_H {
            for x in 0..SURFACE_W {
                let off = (y * SURFACE_W + x) * BYTES_PER_PIXEL;
                let in_bar = x >= bar_x && x < bar_x + 40;
                let (r, g, b) = if in_bar {
                    (216u8, 90u8, 48u8)
                } else {
                    (24u8, 24u8, 30u8)
                };
                buf[off] = r;
                buf[off + 1] = g;
                buf[off + 2] = b;
                buf[off + 3] = 255;
            }
        }
    }
    ffi::RawSurface {
        ptr,
        width: SURFACE_W as u32,
        height: SURFACE_H as u32,
        stride: (SURFACE_W * BYTES_PER_PIXEL) as u32,
    }
}

static PAINTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn on_frame_painted(painted: u32) {
    PAINTED.store(painted, Ordering::Relaxed);
    println!("[rust]  <- C++ 回报第 {painted} 帧已绘制");
}

pub fn on_key(key: i32) -> bool {
    println!("[rust]  <- C++ 上报按键 key={key}");
    true
}

// ════════════════════ main ════════════════════

fn main() {
    let args: Vec<String> = env::args().collect();
    if let Some(code) = rust::history_pager::run(&args) {
        std::process::exit(code);
    }
    let has_flag = |name: &str| args.iter().any(|a| a == name);
    let opt_value = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let profile = opt_value("--config-dir").unwrap_or_default();
    if has_flag("--config-dir") {
        if profile.is_empty() || args.iter().any(|arg| arg.ends_with("-selftest")) {
            eprintln!("--config-dir requires an absolute directory and cannot be combined with self-tests");
            std::process::exit(2);
        }
        if let Err(error) = tiderm_core::set_config_directory(std::path::PathBuf::from(&profile)) {
            eprintln!("--config-dir: {error}");
            std::process::exit(2);
        }
    }
    // C++ self-tests (notably C7 preferences) must never read or write a user's profile.
    if args.iter().any(|arg| arg.ends_with("-selftest")) {
        selftest::prepare_config();
    }

    let selftest = has_flag("--selftest");
    let pty_selftest = has_flag("--pty-selftest");
    let frames: u32 = opt_value("--frames")
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let png_path = opt_value("--png").unwrap_or_default();

    // 自检走 offscreen 平台插件：无 GUI 会话也能出像素级证据。
    if bridge::offscreen_selftest(&args)
        && (has_flag("--offscreen") || env::var_os("QT_QPA_PLATFORM").is_none())
    {
        // SAFETY: startup is single-threaded; Qt and Tokio are initialized below.
        unsafe { env::set_var("QT_QPA_PLATFORM", "offscreen") };
    }

    println!("[rust] native：Rust 引擎 + QtWidgets 视图（cxx 1.0 桥，零 Slint）");
    if let Some(code) = bridge::run_terminal_selftest(&args, &png_path) {
        std::process::exit(code);
    }

    if has_flag("--memory-probe") {
        // 保留的档位就是内存回归的可复现口径（实测数字记在
        // docs/memory-optimization-roadmap.md）：0~10 拆引擎与主窗，12~15 拆主窗/页壳/窗格，
        // 73~75 量"中文进屏花多少钱"，76~78 量"每多一格花多少钱"
        // （详见 cpp/main.cpp: qt_memory_probe）。
        let stage = if has_flag("--pane8-grid") {
            78
        } else if has_flag("--pane8-recycle") {
            79
        } else if has_flag("--pane8-paint") {
            76
        } else if has_flag("--pane8-hidden") {
            77
        } else if has_flag("--solo-ascii") {
            73
        } else if has_flag("--solo-cjk") {
            74
        } else if has_flag("--solo-raster-cjk") {
            75
        } else if has_flag("--shell-only") {
            14
        } else if has_flag("--pane-hidden") {
            15
        } else if has_flag("--view-only") {
            13
        } else if has_flag("--tab-nopump") {
            12
        } else if has_flag("--chrome-terminal") {
            8
        } else if has_flag("--full-no-paint") {
            9
        } else if has_flag("--full-blank-paint") {
            10
        } else if has_flag("--cjk") {
            6
        } else if has_flag("--ascii") {
            7
        } else if has_flag("--render") {
            5
        } else if has_flag("--full") {
            4
        } else if has_flag("--chrome") {
            3
        } else if has_flag("--terminal") {
            2
        } else if has_flag("--font") {
            1
        } else {
            0
        };
        std::process::exit(ffi::qt_memory_probe(stage));
    }

    if has_flag("--welcome-selftest") {
        std::process::exit(ffi::qt_boot_welcome(&png_path));
    }

    // 桥拆域 spike（ADR-014 §Decision 3）：第二桥（bridge/scaffold.rs，文件模块
    // 挂载 + pub use 重导出）Rust→C++→Rust 往返断言，不依赖 Qt。
    if has_flag("--bridge-spike") {
        let expected = rust::spike::spike_rust_id() + 41;
        let got = bridge::scaffold::scaffold_ffi::qt_spike_passthrough(41);
        let ok = got == expected;
        println!(
            "[rust] 桥拆域 spike: {}（qt_spike_passthrough(41)={got:#x}，期望 {expected:#x}）",
            if ok { "通过" } else { "失败" }
        );
        std::process::exit(if ok { 0 } else { 1 });
    }

    if has_flag("--i18n-selftest") {
        std::process::exit(ffi::qt_i18n_selftest());
    }
    if has_flag("--g1-selftest") {
        std::process::exit(g1_selftest());
    }
    if has_flag("--g1-bench") {
        std::process::exit(g1_bench());
    }
    if has_flag("--g2-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_g2(&png_path));
    }
    if has_flag("--g3-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_g3(&png_path));
    }
    if has_flag("--c6-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c6(&png_path));
    }
    if has_flag("--c7-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c7(&png_path));
    }
    if has_flag("--c8-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c8(&png_path));
    }
    if has_flag("--c9-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c9(&png_path));
    }
    if has_flag("--c5-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c5(&png_path));
    }
    if has_flag("--c4-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c4(&png_path));
    }
    if has_flag("--c3-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c3(&png_path));
    }
    if has_flag("--c2-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c2(&png_path));
    }
    if has_flag("--c1-selftest") {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        std::process::exit(ffi::qt_boot_c1(&png_path));
    }
    if has_flag("--window") {
        std::process::exit(ffi::qt_window(&profile));
    }

    let rc = if pty_selftest {
        println!("[rust] Qt 版本（C++ 侧上报）: {}", ffi::qt_version());
        // PowerShell 计算后输出标记行：证明子 shell 跑了且标记停在可视区。
        let feed = if cfg!(windows) {
            r#"powershell -NoProfile -Command "'end-' + (1..500).Count""#
        } else {
            r#"printf 'end-%s\n' $((250 + 250))"#
        };
        ffi::qt_boot_pty(feed, &png_path)
    } else if has_flag("--pty") {
        ffi::qt_run_pty()
    } else if selftest {
        println!(
            "[rust] 像素缓冲 ptr={:#x}，{}×{}（假引擎自检路径）",
            surface_ptr(),
            SURFACE_W,
            SURFACE_H
        );
        ffi::qt_boot(selftest, frames, &png_path)
    } else {
        ffi::qt_window(&profile)
    };

    if selftest {
        let painted = PAINTED.load(Ordering::Relaxed);
        println!("[rust] C++ 回报已绘帧数: {painted}");
        let ok = rc == 0 && painted == frames;
        println!("[rust] 自检结果: {}", if ok { "通过" } else { "失败" });
    }

    std::process::exit(rc);
}
