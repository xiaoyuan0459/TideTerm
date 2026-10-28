//! [`TerminalPane`] — the reusable Rust controller for one terminal pane.
//!
//! It owns the renderer-agnostic grid model ([`TermGrid`]) and a chosen [`PaneRenderer`]
//! (software or GPU), and exposes a small, Wave-2-friendly API. It is deliberately
//! **decoupled from the session transport**: the caller pumps it.
//!
//! ## Lifecycle (how the app-shell drives N of these)
//! 1. Spawn/attach a session through the host's protocol layer sized to your initial
//!    `cols`×`rows`, and construct a `TerminalPane` of the same size.
//! 2. On each `SessionEvent::Data { data, .. }` for this pane → [`feed`](Self::feed), then
//!    drain [`take_replies`](Self::take_replies) and `SessionManager::write` them back
//!    (DSR/DA answers — the shell hangs without them).
//! 3. On a key event → [`crate::keys::encode_key`] (via the host's neutral
//!    [`KeyEvent`](crate::keys::KeyEvent)) → `SessionManager::write`.
//! 4. On a geometry change → compute new `cols`×`rows` from the pixel size and the font
//!    cell metrics, [`resize`](Self::resize) the pane, and `SessionManager::resize` the
//!    session if it returns `true`.
//! 5. Each frame: if [`take_dirty`](Self::take_dirty) (or the cursor blink flipped) →
//!    [`render`](Self::render) and hand the owned
//!    [`PaneSurface`](crate::surface::PaneSurface) to your host adapter.
//!
//! The `Font` is passed in at render time so a whole fleet of panes can share one glyph
//! cache (it is `&mut` because rasterization is lazy/cached).

mod clear;
mod column_selection;
pub mod command_blocks;

#[cfg(test)]
mod file_link_tests;
mod marks;
mod metrics;
mod multi_selection;
pub mod navigation;
mod pointer_selection;
pub mod reading;
mod searching;
pub mod selection_copy;
mod selection_edit;
mod selection_transform;
pub use selection_transform::SelectionEditError;
mod selection_geometry;
mod url_links;
mod viewport;

use crate::file_links::{self, LocalTarget};
use crate::font::Font;
use crate::grid::TermGrid;
use crate::host::Clipboard;
use crate::links::{extract_path_candidates, extract_url_candidates, UrlCandidate};
use crate::render::{PaneRenderer, RenderOpts};
use crate::search::{self, Match};
use crate::selection::{self, Selection};
use crate::surface::PaneSurface;
use std::collections::HashMap;
use std::time::Instant;

/// How long a copy/paste indicator ("toast") stays up, in ms — matches the Electron pane's
/// 1.6s auto-dismiss in `Terminal.tsx`.
const TOAST_MS: u128 = 1600;

/// Minimum pointer travel from the press point (logical px) before a left-press is treated as a
/// drag-select rather than a click. A click frequently twitches a pixel or two; if that twitch
/// straddles a cell boundary the selection would otherwise flip to `dragged` and copy-on-select,
/// clobbering the clipboard right before a paste (the `$c.Dispose()`-rides-along bug). Below this
/// slop the head never tracks, so a click can never copy — matching the few-px dead zone xterm /
/// the Electron pane allow before a drag begins.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// Controller for a single terminal pane: grid model + a pluggable renderer.
pub struct TerminalPane {
    grid: TermGrid,
    reading: reading::Reading,
    commands: command_blocks::Commands,
    marks: crate::marks::Marks,
    navigation: navigation::Navigation,
    /// Reusable snapshot cell buffer, avoiding a per-frame Vec allocation.
    snapshot_cells: Vec<crate::grid::RenderCell>,
    renderer: Box<dyn PaneRenderer>,
    /// This pane's working directory, used to resolve relative path tokens (the renderer-side
    /// half of `core::paths`). `None` falls back to the home dir, matching the pty start dir.
    cwd: Option<String>,
    /// Verified paths cached for this pane's lifetime, keyed by `cwd\x1ftoken`. Only *existing*
    /// paths are cached (negatives aren't), so a file the shell creates becomes clickable on the
    /// next hover — mirroring the Electron renderer's `verified` map.
    verified: HashMap<String, LocalTarget>,
    /// The live drag-selection, if any (our own cell-range model — see [`crate::selection`]).
    /// `None` until a press starts one; a non-dragged selection (a plain click) is held but
    /// renders nothing, so the same press can still resolve to a link click.
    selection: Option<Selection>,
    selection_id: u64,
    selection_serial: u64,
    saved_selections: Vec<multi_selection::SavedSelection>,
    multi_selection: bool,
    selection_locked: bool,
    selection_kind: selection::SelectionKind,
    /// The (logical-px) point of the active selection press, used to gate a real drag from a click
    /// twitch: the head only starts tracking once the pointer moves past [`DRAG_THRESHOLD_PX`] from
    /// here. `None` when no press is in flight.
    select_origin: Option<(f32, f32)>,
    /// Host-injected clipboard service; no platform connection is owned by the engine.
    clipboard: Clipboard,
    /// The transient copy/paste indicator ("toast") + when it was raised; auto-expires after
    /// [`TOAST_MS`]. Drained by [`toast_text`](Self::toast_text).
    toast: Option<(String, Instant)>,
    /// Whether the in-pane search box (Ctrl+F) is open.
    search_shown: bool,
    /// The current search query (the search box text).
    search_query: String,
    /// Search behaviour switches (Aa / .* / word toggles in the search box).
    search_options: search::SearchOptions,
    /// All matches for `search_query` across the grid + scrollback, top to bottom.
    search_matches: Vec<Match>,
    /// Index into `search_matches` of the active (highlighted/revealed) match, if any.
    search_index: Option<usize>,
    /// When the scrollback viewport was last moved by an explicit scroll gesture (wheel /
    /// Shift+PageUp/Down / scroll-to-edge). Drives the vim-style scrollbar's show-then-fade: the
    /// bar is opaque for [`SCROLLBAR_SHOW_MS`], fades over [`SCROLLBAR_FADE_MS`], then is hidden.
    /// `None` once it has fully faded. NOT stamped by the keystroke-driven snap-to-bottom, so the
    /// bar never flashes while you type.
    scroll_activity: Option<Instant>,
    /// The live drag-selection pointer `(x, y, surf_w, surf_h)` in logical px while the button is
    /// held, for edge-autoscroll: when it sits in the top/bottom edge band, the pump's
    /// [`selection_autoscroll_tick`](Self::selection_autoscroll_tick) scrolls the viewport and
    /// grows the selection into off-screen scrollback. `None` when no drag is in flight.
    drag_pointer: Option<(f32, f32, f32, f32)>,
    /// Line-number gutter strip width in TEXT cells (0 = gutter off), as of the last
    /// [`render`](Self::render). Hit-testing subtracts this many cells from pointer x.
    gutter_cells: usize,
}

/// How long the scrollbar stays fully opaque after a scroll gesture before it begins to fade.
const SCROLLBAR_SHOW_MS: u128 = 900;
/// How long the scrollbar takes to fade out once the show window elapses.
const SCROLLBAR_FADE_MS: u128 = 350;
/// Minimum scrollbar thumb height (logical px) so it stays grabbable/visible on a huge buffer.
const SCROLLBAR_MIN_THUMB_PX: f32 = 24.0;
/// Lines scrolled per wheel notch (mirrors the widget's `scroll-requested(±3)` magnitude) — used
/// to collapse a notch back to one mouse-wheel report when forwarding to a mouse-grabbing app.
const WHEEL_LINES_PER_NOTCH: i32 = 3;

/// A link under the cursor — an on-disk-verified path or an http/https URL: where to draw the
/// hover underline (in the pane's *logical* pixel space) plus the target. Returned by
/// [`TerminalPane::link_at`].
#[derive(Debug, Clone, PartialEq)]
pub struct LinkHit {
    /// Underline rect in logical px within the pane surface.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    /// Absolute, verified path the link points at — or the URL itself when [`is_url`](Self::is_url).
    pub abs_path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    /// Tooltip label (`abs_path` with any `:line[:col]` suffix appended; the URL verbatim).
    pub tip: String,
    /// `true` for an http/https URL (opens in the default browser, never disk-verified).
    pub is_url: bool,
}

mod clipboard;
mod drawing;
mod feedback;
mod file_paths;
mod input_events;
mod lifecycle;

impl TerminalPane {
    pub fn set_clipboard(&mut self, access: Box<dyn crate::host::ClipboardAccess>) {
        self.clipboard.install(access);
    }
    /// Create a pane of `cols`×`rows` cells driving the given renderer. Use
    /// [`crate::render::SoftwareRenderer`] (always available) or
    /// [`crate::render::GpuRenderer`] (when a wgpu device is in hand).
    pub fn new(cols: usize, rows: usize, renderer: Box<dyn PaneRenderer>) -> Self {
        Self::with_history(
            cols,
            rows,
            renderer,
            crate::grid::DEFAULT_LIVE_WINDOW,
            crate::grid::DEFAULT_COMPRESSED_CAP,
        )
    }

    /// Create a pane with explicit history budgets (see [`crate::grid::TermGrid::with_history`]):
    /// `live_window` raw lines before rows roll into the compressed store, `compressed_cap`
    /// bytes the compressed long-history store may retain (oldest rows dropped to honour it).
    pub fn with_history(
        cols: usize,
        rows: usize,
        renderer: Box<dyn PaneRenderer>,
        live_window: usize,
        compressed_cap: usize,
    ) -> Self {
        Self {
            grid: TermGrid::with_history(cols, rows, live_window, compressed_cap),
            snapshot_cells: Vec::new(),
            renderer,
            cwd: None,
            verified: HashMap::new(),
            selection: None,
            selection_id: 0,
            selection_serial: 0,
            saved_selections: Vec::new(),
            multi_selection: false,
            selection_locked: false,
            selection_kind: selection::SelectionKind::Stream,
            select_origin: None,
            clipboard: Clipboard::new(),
            toast: None,
            search_shown: false,
            marks: Default::default(),
            navigation: Default::default(),
            reading: reading::Reading::default(),
            commands: command_blocks::Commands::default(),
            search_query: String::new(),
            search_options: search::SearchOptions::default(),
            search_matches: Vec::new(),
            search_index: None,
            scroll_activity: None,
            drag_pointer: None,
            gutter_cells: 0,
        }
    }
}

/// Compute the cell grid that fits a pane of `width_px`×`height_px` *physical* pixels for
/// a font with `cell_w`×`cell_h` cells. Clamped to a sane minimum so a collapsed pane
/// never produces a 0-sized grid. A small free helper the app-shell can reuse for the
/// geometry→resize step.
pub fn cells_for_px(width_px: f32, height_px: f32, cell_w: u32, cell_h: u32) -> (usize, usize) {
    // NaN/±inf 直接 `as u32` 会饱和成 u32::MAX（Rust 浮点转整型语义），下游
    // alacritty `Row::new` 按列数分配 → 数亿列 → 进程级 OOM abort（2026-09-28
    // 探针实测 8 GiB 请求）。先钳到现实像素上限（65535），NaN 归零走 .max(2) 兜底。
    let width_px = if width_px.is_finite() {
        width_px.clamp(0.0, 65535.0)
    } else {
        0.0
    };
    let height_px = if height_px.is_finite() {
        height_px.clamp(0.0, 65535.0)
    } else {
        0.0
    };
    let cols = ((width_px as u32) / cell_w.max(1)).max(2) as usize;
    let rows = ((height_px as u32) / cell_h.max(1)).max(1) as usize;
    (cols, rows)
}

/// Alternate-scroll arrow keys for a wheel notch in the alternate screen (no mouse mode): one
/// Up/Down per scrollback line in `delta_lines` (positive = up). Encoded as application cursor keys
/// (`ESC O A/B`) when DECCKM is set, else normal (`ESC [ A/B`) — what xterm sends so pagers (less,
/// man) scroll on the wheel. This is the no-mouse-grab leg of the "can't scroll Claude" fix.
fn alt_scroll_arrows(delta_lines: i32, app_cursor: bool) -> Vec<u8> {
    let seq: &[u8] = match (delta_lines > 0, app_cursor) {
        (true, false) => b"\x1b[A",
        (false, false) => b"\x1b[B",
        (true, true) => b"\x1bOA",
        (false, true) => b"\x1bOB",
    };
    seq.repeat(delta_lines.unsigned_abs().max(1) as usize)
}

/// Turn raw clipboard text into the exact bytes to write to the pty for a paste.
///
/// Two transforms, both matching how Windows Terminal feeds a paste to conpty:
/// 1. **Normalize line endings to CR (`\r`).** Windows console input treats CR as Enter; a bare
///    LF (`\n`) is mishandled by conpty/PSReadLine, which strands the caret and fragments a
///    multi-line paste across `>>` continuation prompts. Our selection text joins rows with `\n`,
///    and external clipboards carry `\r\n`/`\n` — all collapse to `\r` here.
/// 2. **Bracket** the payload in `ESC[200~ … ESC[201~` *only* when the app enabled bracketed-paste
///    mode (DECSET 2004 — modern PSReadLine / PowerShell 7). Then the shell inserts it as one
///    literal paste (caret at the end, no premature execution). Old shells (Windows PowerShell 5.1)
///    don't set the mode, so the CR-normalized text is sent bare — still the correct Enter handling.
fn prepare_paste(text: &str, bracketed: bool) -> String {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\u{1b}[200~{normalized}\u{1b}[201~")
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests;
