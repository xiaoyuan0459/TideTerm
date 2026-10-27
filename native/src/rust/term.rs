//! 引擎宿主：标签注册表、TerminalPane、字体及桥面 term_* 实现。
//! Phase C ①：多标签。**全部会话状态在 Rust 侧**（`TABS` 注册表），C++ 只持索引
//! 调 `term_*(idx, …)` —— 铁律 2 的机械延续（C++ 不持业务状态）。
//! 引擎是 UI 线程对象（widget 内部 `Rc`，非 `Send`）——所有桥调用都来自 Qt 主线程，
//! `thread_local` 即正确的并发域（与 G4-A RowProvider 定稿同款结论）。
//! 字形缓存跨标签共享。
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use tiderm_terminal::{cells_for_px, Font, PaneSurface, RenderOpts, TerminalPane};

use super::session::SessionHost;
use crate::ffi::{RawSurface, Rect};
use crate::rust::store::session_config::SessionConfig;

pub struct TermState {
    /// 会话 uid（"tab-{id}"，与 BackendEvent 的 id 同源——事件路由键）。
    pub uid: String,
    pub pane: TerminalPane,
    pub opts: RenderOpts,
    pub cursor: super::term_actions::CursorBlink,
    pub reading: bool,
    pub host: SessionHost,
    pub config: SessionConfig,
    pub context: super::term_context::DirectoryContext,
    pub system_monitor: Option<super::system_monitor::MonitorPoller>,
    pub(super) codec: super::session_codec::SessionCodec,
    pub(super) bell_pending: bool,
    pub annotations: super::term_annotations::Annotations,
    pub palette: [[u8; 3]; 16],
    pub generation: u32, // reconnect 时递增，旧传输事件随旧 uid 丢弃
    pub auto_attempts: u32,
    pub auto_retry_at: Option<std::time::Instant>,
    pub manual_stop: bool,
    pub surface: PaneSurface,
    pub viewport_px: (f32, f32),
    /// OSC 0;2 title; unnamed sessions fall back to their configured shell.
    pub title: String,
    /// 状态文本（错误 / 断开 / 自动重连进度；空 = 正常）。是否为错误由
    /// [`TermState::error`] 决定 —— R-19 之前两者混在一个字符串里，导致
    /// 手动「断开」被角标显示成 ⚠ 异常（4=disconnected 从菜单路径不可达）。
    pub note: RefCell<String>,
    /// `note` 的性质：true = 错误（⚠ / 类型码 5），false = 状态文案（断开、重连中）。
    pub error: Cell<bool>,
    /// 会话输出日志（⑧ session:log-*；ring 捕获开关见 rust/log.rs）。
    pub log: super::log::SessionLog,
    pub redraw_pending: bool,
    pub preview_revision: u64,
    pub input_revision: u64,
    pub terminal_input: Option<super::terminal_completion::PendingInput>,
    pub completion_sources: super::completion_sources::Sources,
    pub send_plan: Option<super::multiline::Plan>,
    pub mark_baseline: Option<Vec<tiderm_terminal::marks::MarkSpec>>,
    pub(super) history_export: Option<super::history_export::Export>,
    pub(super) tmux_request: Option<super::tmux_session::Request>,
}

thread_local! {
    /// 稳定 id → 标签。id 单调递增、**永不移位**：C++ 侧 PtyView 持有的 id 在
    /// 其他标签关闭后依然有效（Vec::remove 的移位是 G1 时代单标签假设的残留，
    /// Phase C ① 首跑即崩 0xC0000409 —— with_tab panic 跨 FFI unwind = fail-fast）。
    pub(super) static TABS: RefCell<HashMap<u32, TermState>> = RefCell::new(HashMap::new());
    pub(super) static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    pub(super) static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    pub(super) static FONT_SIZE: Cell<u32> = const { Cell::new(14) };
    /// 设备像素比（HiDPI）。桥面坐标一律**逻辑像素**；引擎内部按 `FONT_SIZE * SCALE`
    /// 的物理像素光栅化字形，故 surface 是物理分辨率（C++ 侧给 QImage 设
    /// devicePixelRatio 后 1:1 落屏，避免逻辑分辨率被放大成马赛克）。输入坐标 ×SCALE
    /// 送 pane，输出矩形 ÷SCALE 回逻辑——SCALE 只活在引擎内，桥面契约不变。
    pub(super) static SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// 当前设备像素比（逻辑→物理）。
pub(super) fn scale() -> f32 {
    SCALE.with(Cell::get)
}

/// 舰队共享字形缓存（font.rs 设计意图：整队 panes 共一份 swash 缓存）。
pub(super) fn with_font<R>(f: impl FnOnce(&mut Font) -> R) -> R {
    FONT.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            // 族名与路径同源（term_ops::font_file）：壳侧 QFont 也问它要，格位与字形
            // 才不会各选各的。
            *slot = Some(
                Font::from_path_index(
                    super::term_ops::font_file(),
                    super::term_font::selected().index,
                    FONT_SIZE.with(|v| v.get() as f32) * SCALE.with(Cell::get),
                )
                .expect("font load"),
            );
        }
        f(slot.as_mut().expect("font initialized"))
    })
}

pub(super) fn font_metrics() -> (u32, u32) {
    with_font(|f| (f.cell_w, f.cell_h))
}

/// 以 `f` 访问标签 `idx` 的引擎状态。索引越界 panic（桥面契约：C++ 不持状态，
/// 越界 = C++ 侧 bug，显式暴露而非静默）。
pub(super) fn with_tab<R>(idx: u32, f: impl FnOnce(&mut TermState) -> R) -> R {
    TABS.with(|c| {
        let mut m = c.borrow_mut();
        let st = m
            .get_mut(&idx)
            .unwrap_or_else(|| panic!("tab {idx} 不存在（C++ 侧 id 失配）"));
        f(st)
    })
}

/// Optional notifications can arrive while Qt retires a view after its session closes.
pub(super) fn try_with_tab<R>(idx: u32, f: impl FnOnce(&mut TermState) -> R) -> Option<R> {
    TABS.with(|tabs| tabs.try_borrow_mut().ok()?.get_mut(&idx).map(f))
}

// ════════════════════ 标签生命周期（Phase C ①） ════════════════════
/// 新建标签（默认本地 shell），返回**稳定 id**（单调递增，不移位）。
pub fn term_new_tab() -> u32 {
    open_tab_with_config(&SessionConfig::blank(
        crate::rust::store::session_config::SessionKind::Local,
    ))
}

/// 按会话配置开标签（Phase C ④：五协议——本地 PTY 即起，远程异步连接经
/// DELIVERY 交付）。返回稳定 id。
pub use super::term_create::open_tab_with_config;

/// 会话 uid（"tab-{id}"——SFTP attach/事件路由键；C++ 侧文件管理器绑定用）。
pub fn term_uid(idx: u32) -> String {
    with_tab(idx, |st| st.uid.clone())
}

/// 按 uid 取会话协议句柄（SFTP 域开 channel 用）。None = 不存在/未交付。
pub(super) fn session_options_of_uid(
    uid: &str,
) -> Option<tiderm_core::session_options::SessionOptions> {
    TABS.with(|tabs| {
        tabs.borrow()
            .values()
            .find(|st| st.uid == uid)
            .map(|st| st.config.meta.options.clone())
    })
}

pub fn protocol_of_uid(uid: &str) -> Option<std::sync::Arc<dyn tiderm_core::protocol::Protocol>> {
    TABS.with(|c| {
        c.borrow()
            .values()
            .find(|st| st.uid == uid)
            .and_then(|st| st.host.protocol())
    })
}

pub fn sftp_allowed(uid: &str) -> bool {
    TABS.with(|tabs| {
        tabs.borrow().values().any(|st| {
            st.uid == uid
                && st.config.kind == super::store::SessionKind::Ssh
                && st.config.meta.sftp_enabled
        })
    })
}

/// 按 uid 应答键盘交互挑战（C++ kb 对话框 → 该会话协议）。
pub fn respond_kb(uid: &str, responses: Vec<String>) -> Result<(), String> {
    TABS.with(|c| {
        c.borrow()
            .values()
            .find(|st| st.uid == uid)
            .ok_or_else(|| format!("会话 {uid} 不存在"))?
            .host
            .respond_kb(responses)
    })
}

/// 关闭标签（drop 会话宿主 → PTY 会话随之终止）。返回剩余数。
/// R-10：C++ 快速连点/竞态下同一 id 可能关两次 —— 幂等返回剩余数，
/// panic 会跨 cxx 变 0xC0000409 整进程消失（同类历史崩溃见 tab id 失配注释）。
pub fn term_close_tab(idx: u32) -> u32 {
    super::sync_channels::retire(idx);
    TABS.with(|c| {
        let mut tabs = c.borrow_mut();
        let Some(uid) = tabs.get(&idx).map(|st| st.uid.clone()) else {
            return tabs.len() as u32;
        };
        super::sftp::close_session(&uid);
        tabs.remove(&idx);
        tabs.len() as u32
    })
}

pub fn term_tab_count() -> u32 {
    TABS.with(|c| c.borrow().len() as u32)
}

// ════════════════════ 会话泵 / 帧 ════════════════════

/// Drain the shared backend once per window tick, independently of pane count.
pub fn term_pump() {
    let touched = super::events::pump_all();
    TABS.with(|c| {
        let mut tabs = c.borrow_mut();
        for st in tabs.values_mut() {
            st.redraw_pending |= touched.contains(&st.uid);
            let tail = st.host.modem.take_output();
            if !tail.is_empty() {
                st.accept_output(&tail);
                st.redraw_pending = true;
            }
            if let Some(title) = st.pane.take_title() {
                st.title = title;
            }
            st.redraw_pending |= super::command_history::observe_prompt(st);
        }
    });
}

/// Consume only this pane's redraw request; other panes retain their requests.
pub fn term_take_redraw(idx: u32) -> bool {
    TABS.with(|c| {
        let mut tabs = c.borrow_mut();
        // R-10 同源：标签已关但 C++ view 尚未销毁的最后几帧 —— 无重绘，不 panic。
        let Some(st) = tabs.get_mut(&idx) else {
            return false;
        };
        std::mem::take(&mut st.redraw_pending)
    })
}

/// Independent terminal windows and one-shot callers own their backend tick.
pub fn term_poll(idx: u32) -> bool {
    term_pump();
    term_take_redraw(idx)
}

pub fn term_frame(idx: u32) -> RawSurface {
    with_tab(idx, |st| {
        if st.pane.take_dirty() || st.surface.width() <= 1 || st.surface.as_cpu().is_none() {
            with_font(|font| {
                let pane = &mut st.pane;
                st.surface = pane.render(font, &st.opts);
            });
        }
        let cpu = st
            .surface
            .as_cpu()
            .expect("software renderer yields cpu surface");
        let raw = cpu.as_raw();
        RawSurface {
            ptr: raw.ptr as usize,
            width: raw.w,
            height: raw.h,
            stride: raw.stride,
        }
    })
}

// ════════════════════ 视口 → 引擎 ════════════════════
pub fn term_resize_px(idx: u32, w: f32, h: f32) -> bool {
    // 布局过渡期（挂入 QSplitter/show 前）会来 0×0 resize——据此重排会把网格
    // 压成 2×1 并触发 ConPTY 对 1 行终端的整屏擦除风暴（Phase C ② 实测），忽略。
    // NaN 与 0 比较恒为 false，能绕过旧守卫直透 `f32 as u32` 的饱和转换（→u32::MAX
    // → 数亿列 → OOM abort，2026-09-28 探针复现），故一并按"非有限值/超现实尺寸"拒绝。
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 || w > 65535.0 || h > 65535.0 {
        return false;
    }
    with_tab(idx, |st| {
        st.viewport_px = (w, h);
        let s = scale();
        let (cw, ch) = font_metrics();
        let (cols, rows) = cells_for_px(w * s, h * s, cw, ch);
        // 与 TermGrid::resize 同值的 2 列下限（alacritty 1 列 reflow 死循环，见彼处注释）。
        let cols = cols
            .saturating_sub(st.pane.gutter_cells(st.opts.gutter))
            .max(2);
        if (cols, rows) != st.pane.grid_size() && st.pane.resize(cols, rows) {
            st.preview_revision = st.preview_revision.wrapping_add(1);
            st.host.resize(cols as u16, rows as u16);
            return true;
        }
        false
    })
}

pub use super::term_input::{term_key, term_write};

pub use super::term_pointer::{term_mouse, term_mouse_send, term_wheel};

// ════════════════════ 选区（G1-4） ════════════════════

pub fn term_selection_begin(idx: u32, x: f32, y: f32) {
    with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane.selection_begin(x * s, y * s, w * s, h * s);
    })
}

pub fn term_selection_update(idx: u32, x: f32, y: f32) {
    with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane.selection_update(x * s, y * s, w * s, h * s);
    })
}

pub fn term_selection_end(idx: u32) -> u32 {
    with_tab(idx, |st| {
        let dragged = st.pane.selection_is_drag();
        st.pane.end_selection_drag();
        if dragged {
            if st.config.meta.options.copy_on_select {
                st.pane.copy_selection().unwrap_or(0) as u32
            } else {
                0
            }
        } else {
            if !st.pane.has_selection() {
                st.pane.selection_clear();
            }
            0
        }
    })
}

pub fn term_paste_from_clipboard(idx: u32) -> bool {
    if super::term_ops::input_locked() {
        return false;
    }
    let text = with_tab(idx, |st| st.pane.paste_from_clipboard());
    if let Some(text) = text {
        super::term_reading::term_paste(idx, text.as_bytes())
    } else {
        false
    }
}

pub fn term_clipboard_text(idx: u32) -> String {
    with_tab(idx, |st| st.pane.paste_from_clipboard().unwrap_or_default())
}

// ════════════════════ 滚动 / 回滚（G1-5） ════════════════════

pub fn term_scroll_page(idx: u32, up: bool) {
    with_tab(idx, |st| st.pane.scroll_page(up));
}

pub fn term_scroll_edge(idx: u32, top: bool) {
    with_tab(idx, |st| {
        if top {
            st.pane.scroll_to_top();
        } else {
            st.pane.scroll_to_bottom();
        }
    })
}

// ════════════════════ 渲染选项（G1-3 / G1-6） ════════════════════

pub fn term_set_render_opts(idx: u32, cursor_style: u8, gutter: bool) {
    let (w, h) = with_tab(idx, |st| {
        st.opts.cursor_style = cursor_style;
        st.opts.gutter = gutter;
        // Invalidate an explicitly requested raster frame without allocating one
        // for Qt's normal text-span path. The next term_frame renders lazily.
        st.surface = PaneSurface::default();
        st.redraw_pending = true;
        st.viewport_px
    });
    term_resize_px(idx, w, h);
}

// ════════════════════ G2 IME ════════════════════

/// 光标单元格 → 像素矩形（surface 坐标；offscreen 下 = 视口坐标）。
pub fn term_caret_rect(idx: u32) -> Rect {
    with_tab(idx, |st| {
        let (col, row) = st.pane.caret_cell().unwrap_or((0, 0));
        let (cw, ch) = font_metrics();
        let s = scale();
        Rect {
            x: (col + st.pane.gutter_cells(st.opts.gutter)) as f32 * cw as f32 / s,
            y: row as f32 * ch as f32 / s,
            w: cw as f32 / s,
            h: ch as f32 / s,
        }
    })
}

pub fn term_ime_commit(idx: u32, text: &str) {
    if !text.is_empty() {
        term_write(idx, text.as_bytes());
    }
}

// 自检/基准辅助（term_feed_inject / term_screen_text / term_cpu_bytes /
// term_stats / term_feed_render_bench）在 `term_diag.rs`（行数门禁拆分）。

// 标题/注册表快照/状态文本等纯查询面在 `term_diag.rs`（行数门禁拆分）。

/// 注册表底层访问（term_diag::registry_snapshot 用；TABS 为本模块私有）。
pub(super) fn registry_values<R>(f: impl FnOnce(&HashMap<u32, TermState>) -> R) -> R {
    TABS.with(|c| f(&c.borrow()))
}
