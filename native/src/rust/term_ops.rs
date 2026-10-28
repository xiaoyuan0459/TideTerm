//! Terminal selection and search actions used by the Qt menu and paint overlay.

use super::term::{font_metrics, scale, with_font, with_tab, FONT, FONT_SIZE, SCALE, TABS};
use crate::ffi::Rect;
use std::cell::Cell;
use tiderm_terminal::{cells_for_px, PaneSurface};

thread_local! {
    static INPUT_LOCKED: Cell<bool> = const { Cell::new(false) };
}

pub fn term_set_locked(locked: bool) {
    INPUT_LOCKED.with(|state| state.set(locked));
    if locked {
        TABS.with(|tabs| {
            for st in tabs.borrow_mut().values_mut() {
                st.input_revision = st.input_revision.wrapping_add(1);
                st.send_plan = None;
                st.history_export = None;
                st.tmux_request = None;
                st.context.shell.clear();
                st.completion_sources.cancel();
            }
        });
    }
}

pub(crate) fn input_locked() -> bool {
    INPUT_LOCKED.with(Cell::get)
}

/// `#RRGGBB` / `#RRGGBBAA` → `[u8;3]`（终端调色板只要 RGB）。
pub(crate) fn parse_rgb(hex: &str) -> [u8; 3] {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0);
    if h.len() >= 6 {
        [byte(0), byte(1), byte(2)]
    } else {
        [0, 0, 0]
    }
}

/// 指定主题的 base16（16 色 ANSI 调色板；引擎约定 index 0=默认背景、7=默认前景）。
pub fn theme_base16_for(id: &str) -> [[u8; 3]; 16] {
    let t = crate::theme::by_id(id);
    let mut out = [[0u8; 3]; 16];
    for (slot, hex) in out.iter_mut().zip(t.base16) {
        *slot = parse_rgb(hex);
    }
    out
}

/// 当前主题的 base16。
pub fn theme_base16() -> [[u8; 3]; 16] {
    theme_base16_for(&crate::theme::prefs_load().theme)
}

/// 主题 base16 下发到全部窗格（对齐批次 A：终端正文配色随主题，
/// 与 chrome 的 QSS/`prefs_theme_colors` 同一信源）。
pub fn term_apply_palette() {
    let base = theme_base16();
    TABS.with(|tabs| {
        for state in tabs.borrow_mut().values_mut() {
            state.palette = base;
            state.pane.set_palette(base);
        }
    });
}

/// 当前主题 base16 的 48 字节平面视图（自检用：验证 C++ `ui_colors().term_bg/term_fg`
/// 与下发给引擎的调色板同源，防止 chrome 与终端正文分叉）。
pub fn term_palette_probe() -> Vec<u8> {
    theme_base16().concat().to_vec()
}

/// Explicit sender targets must not expand through synchronized keyboard input.
pub fn term_write_sessions(ids: &[u32], bytes: &[u8], raw: bool) -> u32 {
    if bytes.is_empty() || input_locked() {
        return 0;
    }
    TABS.with(|tabs| {
        let mut delivered = 0;
        for (&id, state) in tabs.borrow_mut().iter_mut() {
            if ids.contains(&id) && !state.reading && !state.host.modem.busy() && state.host.alive()
            {
                let admission = if raw {
                    state.host.queue_wire(bytes)
                } else {
                    state.host.queue_text(bytes)
                };
                if admission.is_err() {
                    continue;
                }
                super::terminal_completion::record_edit(
                    state,
                    0,
                    if raw { 4 } else { 0 },
                    std::str::from_utf8(bytes).unwrap_or("\0"),
                );
                if !raw {
                    super::command_history::submitted(state, bytes);
                }
                delivered += 1;
            }
        }
        delivered
    })
}

pub fn term_font_px() -> u32 {
    FONT_SIZE.with(Cell::get)
}

/// 字号/缩放变更后：按当前字体度量（物理）与逻辑视口重排所有标签网格并失效 surface。
fn relayout_all() {
    let (cw, ch) = font_metrics();
    let s = scale();
    TABS.with(|tabs| {
        for state in tabs.borrow_mut().values_mut() {
            let (w, h) = state.viewport_px;
            let (cols, rows) = cells_for_px(w * s, h * s, cw, ch);
            let cols = cols
                .saturating_sub(state.pane.gutter_cells(state.opts.gutter))
                .max(1);
            if state.pane.resize(cols, rows) {
                state.host.resize(cols as u16, rows as u16);
            }
            state.surface = PaneSurface::default();
            state.redraw_pending = true;
        }
    });
}

pub fn term_set_font_px(px: u32) -> bool {
    if !(10..=32).contains(&px) {
        return false;
    }
    if px == term_font_px() {
        return true;
    }
    FONT_SIZE.with(|size| size.set(px));
    FONT.with(|cache| *cache.borrow_mut() = None);
    relayout_all();
    true
}

/// 设置设备像素比（HiDPI）。逻辑字号不变，物理光栅化分辨率随 SCALE 改变 → 重排所有
/// 标签网格。与当前值相同则无操作（避免每次 show/paint 重复重置字体与网格）。
pub fn term_set_scale(scale_factor: f32) -> bool {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return false;
    }
    if (scale_factor - scale()).abs() < f32::EPSILON {
        return true;
    }
    SCALE.with(|s| s.set(scale_factor));
    FONT.with(|cache| *cache.borrow_mut() = None);
    relayout_all();
    true
}

/// 引擎与壳**共用**的终端字体选择：路径决定 Rust 侧取整出来的单元格度量，族名决定
/// 壳侧 QFont 画字用哪张脸 —— 必须同源，否则格位与字形各选各的会错位。
pub fn font_file() -> &'static str {
    &super::term_font::selected().path
}

/// 过桥给 C++ 的字体族名（QFont 认名字不认路径）。
pub fn term_font_family() -> String {
    super::term_font::selected().family.clone()
}

/// 生产绘制面：把可见网格采集成**无像素**绘制记录流（线格式见 vendor `spans.rs`）。
/// 取代 `term_frame` 成为 PtyView 的像素来源 —— 字形改由 Qt 文本栈光栅化，
/// Rust 侧不再持有每窗格两份全屏 RGBA 缓冲。
pub fn term_spans(idx: u32) -> Vec<u32> {
    with_tab(idx, |st| {
        with_font(|font| {
            let mut spans = st.pane.spans(font, &st.opts);
            let palette = st.palette;
            let color = |rgb: [u8; 3]| u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255]);
            st.annotations.observe(
                &mut spans,
                st.pane.gutter_cells(st.opts.gutter) as u32 * font.cell_w,
                st.pane.caret_cell().map(|(_, row)| row as u32).unwrap_or(0),
                font.cell_h,
                color(palette[14]),
                color(palette[13]),
            );
            super::term_highlight::accent_visible(
                &mut spans,
                font.cell_w,
                st.pane.gutter_cells(st.opts.gutter) as u32 * font.cell_w,
                palette,
            );
            spans
        })
    })
}

pub fn broadcast_others(origin: u32, bytes: &[u8]) {
    if bytes.is_empty() || input_locked() {
        return;
    }
    if super::term::try_with_tab(origin, |st| {
        st.reading || st.host.modem.busy() || !st.host.alive()
    })
    .unwrap_or(true)
    {
        return;
    }
    let recipients = super::sync_channels::recipients(origin);
    if recipients.is_empty() {
        return;
    }
    TABS.with(|tabs| {
        for (&id, state) in tabs.borrow_mut().iter_mut() {
            if recipients.contains(&id)
                && !state.reading
                && !state.host.modem.busy()
                && state.host.alive()
            {
                super::terminal_completion::record_edit(
                    state,
                    0,
                    0,
                    std::str::from_utf8(bytes).unwrap_or("\0"),
                );
                state.host.write(bytes);
            }
        }
    });
}

fn rect(raw: (f32, f32, f32, f32)) -> Rect {
    let s = scale();
    Rect {
        x: raw.0 / s,
        y: raw.1 / s,
        w: raw.2 / s,
        h: raw.3 / s,
    }
}

pub fn term_select_all(idx: u32) {
    with_tab(idx, |st| {
        st.pane.select_all();
        st.pane.copy_selection();
        st.redraw_pending = true;
    });
}

pub fn term_selected_text(idx: u32) -> String {
    with_tab(idx, |st| st.pane.selection_text().unwrap_or_default())
}

pub fn term_deselect(idx: u32) {
    with_tab(idx, |st| {
        st.pane.selection_clear();
        st.redraw_pending = true;
    });
}

pub fn term_selection_rects(idx: u32) -> Vec<Rect> {
    with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane
            .selection_rects(w * s, h * s)
            .into_iter()
            .map(rect)
            .collect()
    })
}

pub fn term_search_set(idx: u32, query: &str, flags: u32) -> String {
    let options = tiderm_terminal::search::SearchOptions::from_flags(flags);
    if let Some(error) = tiderm_terminal::search::validate_query(query, options) {
        return format!("无效的正则表达式：{error}");
    }
    with_tab(idx, |st| {
        st.pane.search_open();
        st.pane.search_set_options(options);
        st.pane.search_set_query(query);
        st.redraw_pending = true;
        search_label(st.pane.search_count())
    })
}

pub fn term_search_query(idx: u32) -> String {
    with_tab(idx, |st| st.pane.search_query().to_owned())
}

pub fn term_search_flags(idx: u32) -> u32 {
    with_tab(idx, |st| st.pane.search_options().to_flags())
}

pub fn term_search_step(idx: u32, forward: bool) -> String {
    with_tab(idx, |st| {
        st.pane.search_step(forward);
        st.redraw_pending = true;
        search_label(st.pane.search_count())
    })
}

fn search_label((current, total): (usize, usize)) -> String {
    format!("查找结果 {current}/{total}")
}

pub fn term_search_rects(idx: u32) -> Vec<Rect> {
    with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane
            .search_view_rects(w * s, h * s)
            .0
            .into_iter()
            .map(rect)
            .collect()
    })
}

pub fn term_search_active_rect(idx: u32) -> Rect {
    with_tab(idx, |st| {
        let (w, h) = st.viewport_px;
        let s = scale();
        st.pane
            .search_view_rects(w * s, h * s)
            .1
            .map(rect)
            .unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            })
    })
}
