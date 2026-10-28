//! Create panes from configuration and duplicate the active session.
use super::session::SessionHost;
use super::term::{font_metrics, scale, TermState, NEXT_ID, TABS};
use crate::rust::store::session_config::SessionConfig;
use std::cell::Cell;
use std::cell::RefCell;
use tiderm_terminal::{
    cells_for_px, PaneSurface, RenderOpts, SoftwareRenderer, TerminalPane, GUTTER_CELLS,
};

pub fn open_tab_with_config(cfg: &SessionConfig) -> u32 {
    if let Err(error) = super::session_advanced::validate(cfg) {
        eprintln!("[session] invalid options: {error}");
        return u32::MAX;
    }
    let mut cfg = cfg.clone();
    super::term_context::prepare_local(&mut cfg);
    let cfg = &cfg;
    let id = NEXT_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });
    let (cw, ch) = font_metrics();
    let s = scale();
    let (cols, rows) = cells_for_px(800.0 * s, 500.0 * s, cw, ch);
    // 与 `term_resize_px` 同口径：gutter 在建表时就已开启（见下方 opts），初始列数必须扣掉它。
    // 否则 PTY 先按无 gutter 的宽度启动、随后缩列，启动阶段已回显的输出右端会被裁。
    let cols = cols.saturating_sub(GUTTER_CELLS).max(2);
    let uid = format!("tab-{id}");
    let host = SessionHost::start(uid.clone(), cfg, cols as u16, rows as u16);
    host.resize(cols as u16, rows as u16);
    let mut pane = TerminalPane::with_history(
        cols,
        rows,
        Box::new(SoftwareRenderer::new()),
        tiderm_terminal::grid::DEFAULT_LIVE_WINDOW,
        // 每窗格回滚预算走偏好（P0.1 的旋钮）；曾硬编码 32 MiB 使该设置形同虚设。
        (if cfg.meta.options.scrollback_mb == 0 {
            crate::theme::UiPrefs::load().scrollback_mb
        } else {
            cfg.meta.options.scrollback_mb as u64
        } as usize)
            << 20,
    );
    pane.set_clipboard(super::terminal_host::clipboard());
    super::term_modes::apply(&mut pane, &cfg.meta.options);
    if matches!(
        cfg.kind,
        super::store::SessionKind::Local
            | super::store::SessionKind::Ssh
            | super::store::SessionKind::Telnet
    ) {
        pane.command_startup_begin();
    }
    let mark_baseline = match super::mark_profile::load().and_then(|rules| {
        pane.mark_replace(&rules)
            .map_err(|_| "invalid mark rules".to_string())?;
        Ok(rules)
    }) {
        Ok(rules) => Some(rules),
        Err(_) => {
            eprintln!("[marks] default rules unavailable; pane starts without marks");
            None
        }
    };
    let title = pane.take_title().unwrap_or_default();
    let palette = crate::rust::term_ops::theme_base16();
    pane.set_palette(palette);
    // 远程会话在标题/面包屑上先行露出配置名（连接成功后 OSC 标题接管）。
    let title = if title.is_empty()
        && !matches!(
            cfg.kind,
            crate::rust::store::session_config::SessionKind::Local
        ) {
        cfg.display_name()
    } else {
        title
    };
    TABS.with(|c| {
        c.borrow_mut().insert(
            id,
            TermState {
                uid,
                pane,
                opts: RenderOpts {
                    cursor_on: true,
                    cursor_style: cfg.meta.options.cursor_style,
                    // Qt 宿主固定开启：行号栏同时是时间戳栏的数据源，且初始尺寸已按它扣过列宽。
                    gutter: true,
                },
                cursor: Default::default(),
                reading: false,
                host,
                config: cfg.clone(),
                context: super::term_context::DirectoryContext::new(cfg),
                system_monitor: Default::default(),
                codec: super::session_codec::SessionCodec::new(&cfg.meta.charset),
                bell_pending: false,
                annotations: super::term_annotations::Annotations::default(),
                palette,
                generation: 0,
                auto_attempts: 0,
                auto_retry_at: None,
                manual_stop: false,
                surface: PaneSurface::default(),
                viewport_px: (800.0, 500.0),
                title,
                note: RefCell::new(String::new()),
                error: Cell::new(false),
                log: {
                    let mut log = super::log::SessionLog::new();
                    if cfg.meta.options.log_enabled
                        && let Err(error) = log.start_disk()
                    {
                        eprintln!("[session-log] {error}");
                    }
                    log
                },
                redraw_pending: false,
                preview_revision: 0,
                input_revision: 0,
                terminal_input: None,
                completion_sources: Default::default(),
                send_plan: None,
                mark_baseline,
                history_export: None,
                tmux_request: None,
            },
        );
    });
    id
}
