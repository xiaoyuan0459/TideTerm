//! The terminal grid model — an `alacritty_terminal::Term` fed by a `vte` parser,
//! producing a renderer-agnostic [`GridSnapshot`] for the [`crate::render`] backends.
//!
//! Lifted from Spike A's `term_backend.rs`, but with the **PTY removed**: in the real
//! app the live shell is owned by the host protocol layer, which supplies UTF-8
//! output. This type remains a pure model:
//!
//!   * [`TermGrid::feed`] — advance the parser with raw session bytes.
//!   * [`TermGrid::take_replies`] — drain terminal-originated writes (DSR/DA query
//!     replies). The conpty issues `ESC[6n` at startup and **blocks until answered**, so
//!     the controller must forward these back to the session's pty (via
//!     `SessionManager::write`). Without it the shell hangs with no output.
//!   * [`TermGrid::resize`] — reflow the grid (the pty resize is the session's job).
//!   * [`TermGrid::snapshot`] — a flat, resolved-RGBA grid for any `PaneRenderer`.
//!   * [`TermGrid::take_dirty`] — damage-gated repaint flag.

mod appearance;
mod clearing;
mod export;
mod mailbox;
mod metadata;
mod output;
pub(crate) use output::OutputEvent;
mod presentation;
mod projection;
pub(crate) mod reading;
mod reflow;
mod search_text;
mod viewport;

use crate::scrollback::{LineData, StyleRun};
use alacritty_terminal::grid::{Dimensions, Row, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::{Processor, Rgb};
pub use mailbox::OscEvent;
use std::cell::RefCell;

/// A single resolved cell ready for rasterization.
#[derive(Clone, Copy)]
pub struct RenderCell {
    pub ch: char,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Left half of a wide (CJK) glyph — render glyph, occupies 2 columns.
    pub wide: bool,
    /// Right-half spacer of a wide glyph — skip glyph, keep bg.
    pub wide_spacer: bool,
}

impl Default for RenderCell {
    fn default() -> Self {
        RenderCell {
            ch: ' ',
            fg: [0xc0, 0xca, 0xf5, 0xff],
            bg: [0, 0, 0, 0],
            bold: false,
            italic: false,
            underline: false,
            wide: false,
            wide_spacer: false,
        }
    }
}

/// A flat, renderer-agnostic snapshot of the grid: resolved RGBA per cell so renderers
/// never touch alacritty/vte types.
pub struct GridSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<RenderCell>,
    pub cursor: (usize, usize), // (col, row) in viewport
    pub cursor_visible: bool,
    pub default_bg: [u8; 4],
    pub default_fg: [u8; 4],
    /// Total lines retained in the buffer (store + alacritty history + viewport rows) —
    /// drives the line-number gutter width (digit count).
    pub buffer_lines: u64,
    /// Per-viewport-row 1-based buffer line number (top row of the snapshot first), for
    /// the line-number gutter. Folds skip hidden source numbers; blank projected rows use 0.
    pub row_numbers: Vec<u64>,
}

impl GridSnapshot {
    #[inline]
    pub fn cell(&self, col: usize, row: usize) -> &RenderCell {
        &self.cells[row * self.cols + col]
    }
}

/// Implements `alacritty_terminal::grid::Dimensions` so `Term::new`/`resize` accept our size.
#[derive(Clone, Copy)]
pub struct TermSize {
    pub cols: usize,
    pub rows: usize,
}
impl alacritty_terminal::grid::Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// The terminal grid: a parser-fed `Term` plus a resolved 256-colour palette. Owns no
/// PTY — see the module docs.
pub struct TermGrid {
    term: Term<mailbox::Mailbox>,
    parser: Processor,
    events: mailbox::Mailbox,
    palette: [Rgb; 256],
    size: TermSize,
    projection: projection::Projection,
    dirty: bool,
    /// Compressed long-history store (P0b). alacritty only keeps a small raw window
    /// ([`live_window`](Self::live_window)); rows scrolled past it are drained here in
    /// RLE/deduped form, so long histories cost ~O(compressed) instead of O(cols×24 B×lines).
    store: crate::scrollback::background::BackgroundStore,
    /// Raw `Row<Cell>` window alacritty keeps before draining into `store`.
    live_window: usize,
    /// Product-owned viewport intent, reconciled with the active parser grid.
    viewport: viewport::Viewport,
    /// Small LRU of expanded store rows (P0.2): avoids rebuilding `Vec<RenderCell>` every
    /// frame while scrolling through long history.
    row_cache: RefCell<Vec<(usize, Vec<RenderCell>)>>,
    /// Deferred rows waiting to be compressed into `store` (P2.4). Kept separate from the
    /// store so feed can batch the hash/interner work instead of doing it on every drain.
    pending: Vec<LineData>,
}

/// Live window: how many raw `Row<Cell>` lines alacritty keeps before they are drained into
/// the compressed [`ScrollbackStore`] (P0b, strategy A).
///
/// This is the single most expensive per-pane allocation in a busy session: a raw row is
/// `cols × size_of::<Cell>()` ≈ 24 B/cell *pre-allocated and resident*, while the compressed
/// store holds the same line for a fraction of that. `drain_pending` runs after every history
/// growth, so the raw window never exceeds `live_window + 1` rows and the parser's cap
/// (`2 × live_window`) is only a safety margin — the conversion work per line is identical at
/// any window size, only the resident high-water changes.
///
/// 64 lines ≈ 1.5 screens at a 40-row pane: enough for the viewport plus a little overscroll
/// to stay on the raw path, and 64 × 200 cols × 24 B ≈ 0.3 MB instead of ~1.9 MB at the
/// former 200-line setting.
pub const DEFAULT_LIVE_WINDOW: usize = 200;

/// Byte budget for the compressed long-history store. 32 MB holds ~5·10^4 full 200-col
/// lines compressed (vs ~240 MB of raw alacritty rows) — still the WindTerm "dynamic
/// compression" target, but halved from the 64 MB default to cut steady-state RAM on
/// long-lived sessions / many panes. Configurable per-grid via [`with_history`](Self::with_history).
pub const DEFAULT_COMPRESSED_CAP: usize = 32 * 1024 * 1024;

/// Max rows kept in the expanded-row cache. At 200 columns each entry is roughly
/// `cols * size_of::<RenderCell>()` (~4.8 KB), so 128 entries ≈ 0.6 MB.
const ROW_CACHE_MAX: usize = 128;

/// Flush threshold for deferred scrollback compression (P2.4). `feed` accumulates
/// converted rows in `pending` and only runs the store hash/interner when this many
/// rows are queued (or at the end of the feed call), turning per-window compression
/// into larger batches.
const PENDING_FLUSH_THRESHOLD: usize = 1024;

mod events;
mod history;
mod navigation;

impl TermGrid {
    /// A fresh grid sized to `cols`×`rows` (each clamped to ≥1), using the P0b defaults:
    /// [`DEFAULT_LIVE_WINDOW`] raw lines + [`DEFAULT_COMPRESSED_CAP`] compressed history.
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_history(cols, rows, DEFAULT_LIVE_WINDOW, DEFAULT_COMPRESSED_CAP)
    }

    /// Like [`new`](Self::new) but with explicit window sizes.
    ///
    /// `live_window` is the raw-history drain threshold; the parser normally permits
    /// twice that many raw history rows. Local screen clearing temporarily permits
    /// one whole screen to be archived before restoring that limit. Older rows use
    /// the compressed [`ScrollbackStore`] bounded by `compressed_cap` bytes.
    pub fn with_history(
        cols: usize,
        rows: usize,
        live_window: usize,
        compressed_cap: usize,
    ) -> Self {
        let size = TermSize {
            cols: cols.max(1),
            rows: rows.max(1),
        };
        // Twice the live window so a single feed slice (≤ `live_window` newlines) can never
        // hit alacritty's cap and silently drop the oldest rows before drain_pending runs
        // (peak raw footprint ≈ 2×live_window rows, ~2.4 MB at 200×50 with the 200-line
        // default — still negligible next to the compressed store).
        let config = Config {
            scrolling_history: live_window.max(1) * 2,
            // G6: allow OSC 52 in both directions — store (remote vim/tmux yank → local
            // clipboard) and load (paste from local clipboard into a remote app over ssh).
            // alacritty's default is OnlyCopy; we are a user-facing terminal and the load
            // reply is explicitly requested by the application being pasted into.
            osc52: Osc52::CopyPaste,
            ..Config::default()
        };
        let palette = default_palette();
        let events = mailbox::Mailbox::new(palette[..16].try_into().unwrap());
        let term = Term::new(config, &size, events.clone());
        TermGrid {
            term,
            parser: Processor::new(),
            events,
            palette,
            size,
            projection: projection::Projection::default(),
            dirty: true,
            store: crate::scrollback::background::BackgroundStore::new(compressed_cap),
            live_window: live_window.max(1),
            viewport: viewport::Viewport::default(),
            row_cache: RefCell::new(Vec::new()),
            pending: Vec::new(),
        }
    }

    /// Apply a colour theme by overriding the 16 base ANSI colours (indices 0–15; index 0 is
    /// the default background, index 7 the default foreground). The 6×6×6 colour cube and the
    /// grayscale ramp (indices 16–255) are kept. Marks the grid dirty so it repaints.
    pub fn set_base16(&mut self, base: [[u8; 3]; 16]) {
        for (i, c) in base.iter().enumerate() {
            self.palette[i] = Rgb {
                r: c[0],
                g: c[1],
                b: c[2],
            };
        }
        self.events
            .set_base16(self.palette[..16].try_into().unwrap());
        self.dirty = true;
    }
}

/// SGR 2 (faint/dim) colour: blend `fg` 55% toward `bg` — the common terminal approach
/// (a heavier blend than 50% so dim text reads clearly quieter than normal text while
/// staying legible). Alpha is kept opaque; only the hue moves toward the background.
mod colors;
use colors::*;

#[cfg(test)]
mod tests;
