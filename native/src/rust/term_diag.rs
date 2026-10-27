//! 自检/基准辅助（Phase C ④ 从 term.rs 拆出——行数门禁）。
//!
//! G1/G2/c1–c4 自检与 g1-bench 的确定性查询面：喂入注入、屏文本、像素字节、
//! 引擎统计、独立基准 pane。仅自检/基准路径使用，运行时面在 `term.rs`。

use tiderm_terminal::{RenderOpts, SoftwareRenderer, TerminalPane};

use super::term::{font_metrics, registry_values, with_font, with_tab, TermState};
use crate::ffi::TermStats;

pub fn term_feed_inject(idx: u32, bytes: &[u8]) {
    with_tab(idx, |st| {
        st.accept_output(bytes);
        st.redraw_pending |= !bytes.is_empty();
    });
}

/// 内存探针：引擎侧常驻分量的一行报告（每窗格网格/历史/压缩回滚/帧缓冲 + 全局字形缓存）。
/// 与 `measure-startup.ps1` 的进程级 Private/WorkingSet 对照，才能把"谁吃了多少"说清。
pub fn term_mem_report() -> String {
    use std::fmt::Write;
    let mut s = String::new();
    super::term::registry_values(|tabs| {
        for (id, st) in tabs.iter() {
            let (cols, rows) = st.pane.grid_size();
            let surf_b = st
                .surface
                .as_cpu()
                .map(|c| c.width() as usize * c.height() as usize * 4)
                .unwrap_or(0);
            let _ = write!(
                s,
                "tab{}: {}x{} 历史{}行 回滚{}B/预算{}B 帧{}B; ",
                id,
                cols,
                rows,
                st.pane.total_history(),
                st.pane.scrollback_bytes(),
                st.pane.scrollback_budget(),
                surf_b
            );
        }
    });
    super::term::FONT.with(|c| {
        if let Some(f) = c.borrow().as_ref() {
            let _ = write!(
                s,
                "字形缓存{}项/{}B 回退memo{}项",
                f.cache_len(),
                f.cache_bytes(),
                f.resolve_cache_len()
            );
        }
    });
    s
}

pub fn term_screen_text(idx: u32) -> String {
    with_tab(idx, |st| st.pane.screen_text())
}

/// 当前帧的 RGBA 像素字节（像素互异断言 + PNG 落盘的输入）。
pub fn term_cpu_bytes(idx: u32) -> Option<Vec<u8>> {
    with_tab(idx, |st| {
        let cpu = st.surface.as_cpu()?;
        Some(cpu.as_rgba_bytes().to_vec())
    })
}

pub fn term_stats(idx: u32) -> TermStats {
    with_tab(idx, |st| {
        let (cols, rows) = st.pane.grid_size();
        let (ccol, crow) = st.pane.caret_cell().unwrap_or((0, 0));
        let cpu = st.surface.as_cpu();
        TermStats {
            cols: cols as u32,
            rows: rows as u32,
            cursor_col: ccol as u32,
            cursor_row: crow as u32,
            // 行号栏占的文本格数：`cols` 只是正文列数，光标/内容在屏上都从它右边开始。
            // 自检断言（IME 锚点、网格宽度）必须加上它，否则开着行号栏时全按无栏算。
            gutter_cells: st.pane.gutter_cells(st.opts.gutter) as u32,
            surface_w: cpu.map(|c| c.width()).unwrap_or(0),
            surface_h: cpu.map(|c| c.height()).unwrap_or(0),
            cell_w: font_metrics().0,
            cell_h: font_metrics().1,
        }
    })
}

/// G1-1/G1-8 基准：由 native --g1-bench 与 --g1-selftest 共用
/// （独立临时 pane：`TerminalPane::new` 200×50 + 200 列 ASCII × 200k，
/// feed 全量 + 1000 帧渲染）。返回 行/秒。A/B 基线见 selftest.rs。
pub fn term_feed_render_bench(bursts: u32, lines_per: u32) -> f64 {
    with_font(|font| {
        println!(
            "[bench] font_px={} scale={} cell={}x{}",
            super::term_ops::term_font_px(),
            super::term::scale(),
            font.cell_w,
            font.cell_h
        );
        let opts = RenderOpts {
            cursor_on: true,
            cursor_style: 0,
            gutter: false,
        };
        let mut pane = TerminalPane::new(200, 50, Box::new(SoftwareRenderer::new()));

        let total = (bursts * lines_per) as usize;
        let mut payload = Vec::with_capacity(total * 202);
        for i in 0..total {
            let base = (i * 37) as u8;
            for c in 0..200u32 {
                payload.push(b'a' + ((base + c as u8) % 26));
            }
            payload.extend_from_slice(b"\r\n");
        }
        let t = std::time::Instant::now();
        pane.feed_bytes(&payload);
        for _ in 0..1000 {
            let surface = pane.render(font, &opts);
            if let Some(cpu) = surface.as_cpu() {
                std::hint::black_box(cpu.as_rgba_bytes().len());
            }
        }
        let elapsed = t.elapsed().as_secs_f64();
        total as f64 / elapsed
    })
}

/// Saved identity takes priority; unnamed terminals use OSC or their actual shell.
pub fn term_title(idx: u32) -> String {
    with_tab(idx, |st| display_title(st))
}

/// 注册表快照（pod 域数据源）：全部活跃会话 `(稳定 id, 标题)`，按创建序。
pub fn registry_snapshot() -> Vec<(u32, String)> {
    registry_values(|m| {
        let mut v: Vec<(u32, String)> = m.iter().map(|(id, st)| (*id, display_title(st))).collect();
        v.sort_by_key(|(id, _)| *id); // NEXT_ID 单调 ⇒ id 序 = 创建序
        v
    })
}

/// 连接状态**类型码**（webview `ConnectionStatus` 六态，供 tab 角标/树点取色）：
/// 0=unknown 1=connecting 2=connected 3=running(本地 shell) 4=disconnected 5=error。
/// 与 `term_status` 的文案面同源，避免 C++ 侧解析字符串。
pub fn term_status_kind(idx: u32) -> u8 {
    with_tab(idx, |st| {
        // R-19：只有 error 标志（Error 事件/重连失败/丢帧告警）才归 ⚠ 异常；
        // 手动断开、会话退出、重连排程是状态文案，各归其真实类型码。
        if st.error.get() {
            return 5;
        }
        if st.auto_retry_at.is_some() {
            return 1; // 自动重连排程中 = connecting
        }
        if st.manual_stop {
            // 手动断开后 SessionHost::disconnect 已把 proto 取走（connected()==false），
            // 必须先于 connecting 判定 —— 否则「断开」被显示成「连接中」。
            return 4;
        }
        if !st.host.connected() {
            return 1;
        }
        if !st.host.alive() {
            return 4;
        }
        let local = matches!(
            st.config.kind,
            crate::rust::store::session_config::SessionKind::Local
        );
        u8::from(local) + 2 // local → 3(running)，远程 → 2(connected)
    })
}

/// 渲染后端真名（引擎上报；状态栏「后端」分段用，禁止写死）。
/// 视口回滚偏移（行数）。自检探针：证明 Shift+PgUp 走的是滚动而非发给 pty。
pub fn term_scroll_offset(idx: u32) -> u32 {
    with_tab(idx, |st| {
        u32::try_from(st.pane.scroll_offset()).unwrap_or(u32::MAX)
    })
}

pub fn term_renderer_name(idx: u32) -> String {
    with_tab(idx, |st| st.pane.renderer_name().to_string())
}

pub fn term_status(idx: u32) -> String {
    with_tab(idx, |st| {
        // 错误优先外显（⚠）；其余 note（断开/重连进度）是状态文案，不带 ⚠（R-19）。
        if !st.note.borrow().is_empty() {
            let prefix = if st.error.get() { "⚠ " } else { "" };
            return format!("{prefix}{}", st.note.borrow());
        }
        if !st.host.connected() {
            return format!("{} · 连接中…", display_title(st));
        }
        let (cols, rows) = st.pane.grid_size();
        let (cc, cr) = st.pane.caret_cell().unwrap_or((0, 0));
        let alive = if st.host.alive() { "" } else { " · 已断开" };
        format!(
            "{} · {}×{} cells · cursor ({},{}){}",
            display_title(st),
            cols,
            rows,
            cc,
            cr,
            alive
        )
    })
}

pub(super) fn display_title(st: &TermState) -> String {
    if !st.config.name.is_empty() || !st.config.meta.label.is_empty() {
        return st.config.display_name();
    }
    if !st.title.is_empty() {
        if st.config.kind == super::store::session_config::SessionKind::Local
            && let Some(title) = super::store::session_config::standard_shell_name(&st.title)
        {
            return title.into();
        }
        return st.title.clone();
    }
    if st.config.kind != super::store::session_config::SessionKind::Local {
        return st.config.display_name();
    }
    let shell = st
        .config
        .local
        .as_ref()
        .map(|c| c.shell.as_str())
        .unwrap_or("auto");
    let shell = if matches!(shell, "" | "auto") {
        if cfg!(windows) {
            "powershell".into()
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
        }
    } else {
        shell.to_owned()
    };
    // Executable aliases use the brand spelling; explicit names and arbitrary
    // OSC captions have already returned above with their original spelling.
    super::store::session_config::shell_display_name(&shell)
}
