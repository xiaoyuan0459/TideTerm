//! `tiderm-terminal` — the reusable live-terminal pane component.
//!
//! A UI-framework-neutral terminal controller with explicit host services
//! (`alacritty_terminal` grid → `swash` atlas → software pixel buffer, or optional
//! GPU `wgpu::Texture`). A Qt host can consume text spans or keep the returned
//! [`surface::PaneSurface`] across frames and wrap [`surface::CpuSurface::as_raw`]
//! in a QImage.
//!
//! The pane owns **no PTY**: TideTerm's host protocol layer supplies output and
//! receives key input, resize requests and terminal replies.
//!
//! ## Pieces
//! * [`grid::TermGrid`] — the renderer-agnostic terminal model (fed bytes → snapshot).
//! * [`render`] — the `PaneRenderer` trait + `SoftwareRenderer` / `GpuRenderer`.
//! * [`surface`] — the UI-neutral frame handles (`PaneSurface` / `PixelBuf` / FFI view).
//! * [`font::Font`] — the shared `swash` glyph cache + cell metrics.
//! * [`pane::TerminalPane`] — the controller that ties grid + renderer together.
//! * [`keys::encode_key`] — neutral [`keys::KeyEvent`] → PTY bytes.

mod file_links;
pub mod font;
pub mod grid;
pub mod history_snapshot;
pub mod host;
pub mod keys;
pub mod layout;
pub mod links;
pub mod marks;
mod matching;
pub mod pane;
pub mod render;
pub mod row_provider;
pub mod scrollback;
pub mod search;
pub mod selection;
pub mod spans;
pub mod surface;

// ---- flat re-exports for ergonomic downstream use ----
pub use font::Font;
pub use grid::{GridSnapshot, RenderCell, TermGrid, TermSize};
pub use keys::{encode_key, Key, KeyEvent, Modifiers};
pub use links::{extract_path_candidates, extract_url_candidates, PathCandidate, UrlCandidate};
pub use pane::{cells_for_px, LinkHit, TerminalPane};
#[cfg(feature = "gpu")]
pub use render::GpuRenderer;
pub use render::{gutter_cells, PaneRenderer, RenderOpts, SoftwareRenderer, GUTTER_CELLS};
pub use row_provider::MemRingProvider;
pub use scrollback::{LineData, ScrollbackStore, StyleRun};
pub use selection::{Cell as SelectionCell, Selection};
#[cfg(feature = "gpu")]
pub use surface::GpuSurface;
pub use surface::{CpuSurface, PaneSurface, PixelBuf, RawSurface, Rgba8};
