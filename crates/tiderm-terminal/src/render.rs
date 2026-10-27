//! `PaneRenderer` trait + two implementations.
//!
//! This trait shape is the deliverable the real UI is expected to adopt: a renderer takes
//! a renderer-agnostic `GridSnapshot` plus the shared `Font` and returns an **owned,
//! UI-neutral [`PaneSurface`](crate::surface::PaneSurface)** sized to the pane's
//! *physical* pixel grid (cols*cell_w x rows*cell_h). The host's adapter imports it —
//! Slint zero-copy via `slint_adapter::to_image`, a Qt host via
//! [`CpuSurface::as_raw`](crate::surface::CpuSurface::as_raw).
//!
//!  - `SoftwareRenderer`: swash coverage masks blended into a double-buffered
//!    pixel buffer (Slint build: `SharedPixelBuffer`; the adapter's `from_rgba8`
//!    is a reference-count transfer). The RDP / software-GL fallback.
//!  - `GpuRenderer`: a swash→etagere R8 atlas + instanced quads rendered into a per-pane
//!    `wgpu::Texture` (Rgba8Unorm) on Slint's *own* device, handed out as
//!    `PaneSurface::Gpu` (the Slint `Image::try_from` import moved host-side).
//!
//! This module is UI-framework-free since Phase A (路线 A): no `slint` import remains —
//! the crate compiles `--no-default-features` (Qt route) with zero Slint.

use crate::font::{CachedGlyph, Font, GlyphKey};
use crate::grid::GridSnapshot;
#[cfg(feature = "gpu")]
use crate::surface::GpuSurface;
use crate::surface::{CpuSurface, PaneSurface, PixelBuf, Rgba8};

pub struct RenderOpts {
    pub cursor_on: bool,
    /// Cursor shape: 0 = block (true-invert), 1 = underline (bottom band),
    /// 2 = bar (left band). Underline/bar paint the band in the cell's fg over the
    /// glyph (WindTerm-style outline cursors); values > 2 render as block.
    pub cursor_style: u8,
    /// Draw the line-number gutter (WindTerm-style) left of the text: the surface grows
    /// a fixed [`GUTTER_CELLS`] cells wider and every content pass shifts right. The
    /// gutter numbers also carry the host's per-line timestamps, so turning it off
    /// removes that data source.
    pub gutter: bool,
}

/// Gutter width in whole cells: five digits (the 2^16-line scrollback ceiling) plus one
/// spare cell of separation from the text.
///
/// **Fixed on purpose.** The width used to follow `buffer_lines`, so the gutter grew while
/// the grid kept the `cols` computed at the last resize: the span surface then reserved more
/// cells than the viewport had room for and every row lost its right edge — long commands
/// were cut off instead of wrapping into view. Deriving `cols` from the live line count
/// instead would shrink the grid mid-session and reflow (dropping an in-progress command
/// draft), so the width stays constant for the pane's whole life.
pub const GUTTER_CELLS: usize = 6;

/// Gutter width in whole cells. Always [`GUTTER_CELLS`]; the argument is kept so call sites
/// that only know the buffer size stay unchanged.
pub fn gutter_cells(_buffer_lines: u64) -> usize {
    GUTTER_CELLS
}

/// Dim colour for gutter numbers: `fg` blended 55% toward `bg` (the SGR-2 dim
/// convention — quieter than normal text but still legible).
pub(crate) fn gutter_color(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let mix = |f: u8, b: u8| (b as u16 * 55 / 100 + f as u16 * 45 / 100) as u8;
    [
        mix(fg[0], bg[0]),
        mix(fg[1], bg[1]),
        mix(fg[2], bg[2]),
        0xff,
    ]
}

pub trait PaneRenderer {
    fn name(&self) -> &'static str;
    /// Render `grid` into an owned [`PaneSurface`](crate::surface::PaneSurface) at
    /// physical resolution. Implementations cache internal buffers/atlases across
    /// calls; `surface.clone()` hands the same frame out again by reference count.
    fn render(&mut self, grid: &GridSnapshot, font: &mut Font, opts: &RenderOpts) -> PaneSurface;
}

#[inline]
fn lerp_u8(b: u8, f: u8, cov: u8) -> u8 {
    let b = b as i32;
    let f = f as i32;
    (b + (f - b) * cov as i32 / 255) as u8
}

/// Blend a rasterized glyph's coverage mask into `px` in `color`, with the cell's pen at
/// (`x0`,`y0`) and the baseline at `y0 + ascent`. Shared by the normal glyph pass and the
/// cursor's true-invert redraw so the two never drift. Clips to the `w`×`h` buffer.
#[inline]
// pre-existing; deferred per repo lint policy (test.yml)
#[allow(clippy::too_many_arguments)]
fn blit_glyph(
    px: &mut [Rgba8],
    w: u32,
    h: u32,
    stride: usize,
    g: &CachedGlyph,
    x0: u32,
    y0: u32,
    ascent: i32,
    color: [u8; 4],
) {
    if g.w == 0 {
        return;
    }
    let pen_x = x0 as i32 + g.left;
    let gy0 = y0 as i32 + ascent - g.top;
    for gy in 0..g.h as i32 {
        let dy = gy0 + gy;
        if dy < 0 || dy >= h as i32 {
            continue;
        }
        for gx in 0..g.w as i32 {
            let dx = pen_x + gx;
            if dx < 0 || dx >= w as i32 {
                continue;
            }
            let cov = g.mask[(gy * g.w as i32 + gx) as usize];
            if cov == 0 {
                continue;
            }
            let p = &mut px[dy as usize * stride + dx as usize];
            p.r = lerp_u8(p.r, color[0], cov);
            p.g = lerp_u8(p.g, color[1], cov);
            p.b = lerp_u8(p.b, color[2], cov);
            p.a = 255;
        }
    }
}

// =================================================================================
// Software renderer
// =================================================================================

pub struct SoftwareRenderer {
    bufs: Vec<PixelBuf>,
    idx: usize,
    w: u32,
    h: u32,
}

impl Default for SoftwareRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl SoftwareRenderer {
    pub fn new() -> Self {
        SoftwareRenderer {
            bufs: Vec::new(),
            idx: 0,
            w: 0,
            h: 0,
        }
    }
}

impl PaneRenderer for SoftwareRenderer {
    fn name(&self) -> &'static str {
        "software (swash → pixel buffer)"
    }

    fn render(&mut self, grid: &GridSnapshot, font: &mut Font, opts: &RenderOpts) -> PaneSurface {
        let cw = font.cell_w;
        let ch = font.cell_h;
        // Gutter strip: whole cells of digits, drawn left of the text; every content
        // pass below shifts right by `gw`.
        let gw = if opts.gutter {
            gutter_cells(grid.buffer_lines) as u32 * cw
        } else {
            0
        };
        let w = (grid.cols as u32 * cw + gw).max(1);
        let h = (grid.rows as u32 * ch).max(1);
        if w != self.w || h != self.h || self.bufs.is_empty() {
            self.w = w;
            self.h = h;
            self.bufs = vec![PixelBuf::alloc(w, h), PixelBuf::alloc(w, h)];
        }
        // Double-buffer: write into the buffer the UI is not currently displaying.
        // `make_mut_rgba` only copies when the previous frame is still shared (COW) —
        // the Slint adapter holds it by reference count exactly as `Image` did.
        self.idx ^= 1;
        let buf = &mut self.bufs[self.idx];
        let px = buf.make_mut_rgba();
        let stride = w as usize;
        let bg0 = grid.default_bg;
        for p in px.iter_mut() {
            *p = Rgba8 {
                r: bg0[0],
                g: bg0[1],
                b: bg0[2],
                a: 255,
            };
        }

        // Line-number gutter numbers (dim fg, right-aligned with one cell margin).
        if gw > 0 {
            let dim = gutter_color(grid.default_fg, grid.default_bg);
            let ascent = font.ascent;
            let gcells = (gw / cw) as usize;
            let width = gcells.saturating_sub(1);
            for (row, &n) in grid
                .row_numbers
                .iter()
                .enumerate()
                .filter(|(_, n)| **n != 0)
            {
                let text = format!("{:>width$}", n, width = width);
                let y0 = row as u32 * ch;
                for (di, dch) in text.chars().enumerate() {
                    if dch == ' ' {
                        continue;
                    }
                    let (font_id, gid) = font.resolve(dch);
                    let g = font.rasterize(GlyphKey {
                        font_id,
                        gid,
                        bold: false,
                        italic: false,
                    });
                    let x0 = di as u32 * cw;
                    blit_glyph(px, w, h, stride, g, x0, y0, ascent, dim);
                }
            }
        }

        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let cell = grid.cell(col, row);
                if cell.wide_spacer {
                    continue;
                }
                let x0 = gw + col as u32 * cw;
                let y0 = row as u32 * ch;
                let cell_px_w = if cell.wide { cw * 2 } else { cw };

                // Background fill.
                if cell.bg[3] > 0 {
                    for yy in 0..ch {
                        let row_off = ((y0 + yy) as usize) * stride + x0 as usize;
                        for xx in 0..cell_px_w {
                            if (x0 + xx) >= w {
                                break;
                            }
                            let p = &mut px[row_off + xx as usize];
                            p.r = cell.bg[0];
                            p.g = cell.bg[1];
                            p.b = cell.bg[2];
                            p.a = 255;
                        }
                    }
                }

                // Glyph. A wide (CJK) glyph is centered in its 2-cell span via the layout
                // layer (T1.5) — never left-beam-clamped.
                if cell.ch != ' ' && cell.ch != '\0' {
                    let (font_id, gid) = font.resolve(cell.ch);
                    let ascent = font.ascent;
                    let g = font.rasterize(GlyphKey {
                        font_id,
                        gid,
                        bold: cell.bold,
                        italic: cell.italic,
                    });
                    let gx = crate::layout::pen_x(x0, cw, cell.wide, g.w, g.left);
                    blit_glyph(px, w, h, stride, g, gx, y0, ascent, cell.fg);
                }

                // Underline. Compute in i32 (a negative ascent must not wrap to a huge u32)
                // and clamp WITHIN this cell's band — never to the bottom screen row.
                if cell.underline {
                    let uy =
                        (y0 as i32 + font.ascent + 2).clamp(y0 as i32, (y0 + ch) as i32 - 1) as u32;
                    let row_off = uy as usize * stride + x0 as usize;
                    for xx in 0..cell_px_w {
                        if (x0 + xx) >= w {
                            break;
                        }
                        let p = &mut px[row_off + xx as usize];
                        p.r = cell.fg[0];
                        p.g = cell.fg[1];
                        p.b = cell.fg[2];
                    }
                }
            }
        }

        // Cursor — style per `opts.cursor_style`: block is TRUE invert (paint the block in
        // the cell's fg, then redraw the glyph on top in the colour it sits on (the cell bg,
        // or the grid default bg when the cell is transparent) so the character under the
        // cursor stays visible); underline/bar paint a thin band in the cell's fg over the
        // glyph, leaving the glyph itself untouched.
        if grid.cursor_visible && opts.cursor_on {
            let (col, row) = grid.cursor;
            let ccol = col.min(grid.cols - 1);
            let crow = row.min(grid.rows - 1);
            let x0 = gw + ccol as u32 * cw;
            let y0 = crow as u32 * ch;
            let cell = *grid.cell(ccol, crow);
            let block_w = if cell.wide { cw * 2 } else { cw };
            let style = opts.cursor_style;
            // Band geometry: underline hugs the cell bottom, bar hugs the left edge.
            let (band_x, band_y, band_w, band_h) = if style == 1 {
                let uh = (ch / 5).max(2);
                (x0, y0 + ch.saturating_sub(uh), block_w, uh)
            } else {
                let bw = (cw / 5).max(2);
                (x0, y0, bw, ch)
            };
            // The colour the inverted glyph is drawn in: the cell's own bg if opaque, else the
            // grid default bg — i.e. whatever the block colour "replaced", so it reads as invert.
            let under = if cell.bg[3] > 0 {
                cell.bg
            } else {
                grid.default_bg
            };

            if style == 0 {
                // 1) Fill the cursor block with the cell's fg colour.
                for yy in 0..ch {
                    if y0 + yy >= h {
                        break;
                    }
                    let row_off = (y0 + yy) as usize * stride + x0 as usize;
                    for xx in 0..block_w {
                        if x0 + xx >= w {
                            break;
                        }
                        let p = &mut px[row_off + xx as usize];
                        p.r = cell.fg[0];
                        p.g = cell.fg[1];
                        p.b = cell.fg[2];
                        p.a = 255;
                    }
                }

                // 2) Redraw the glyph over the block in `under` so it inverts cleanly. A wide
                //    glyph is centered in its 2-cell span (same layout call as the normal pass).
                if cell.ch != ' ' && cell.ch != '\0' {
                    let (font_id, gid) = font.resolve(cell.ch);
                    let ascent = font.ascent;
                    let g = font.rasterize(GlyphKey {
                        font_id,
                        gid,
                        bold: cell.bold,
                        italic: cell.italic,
                    });
                    let gx = crate::layout::pen_x(x0, cw, cell.wide, g.w, g.left);
                    blit_glyph(px, w, h, stride, g, gx, y0, ascent, under);
                }
            } else {
                // Underline / bar: paint the band in the cell's fg over the glyph.
                for yy in 0..band_h {
                    if band_y + yy >= h {
                        break;
                    }
                    let row_off = (band_y + yy) as usize * stride + band_x as usize;
                    for xx in 0..band_w {
                        if band_x + xx >= w {
                            break;
                        }
                        let p = &mut px[row_off + xx as usize];
                        p.r = cell.fg[0];
                        p.g = cell.fg[1];
                        p.b = cell.fg[2];
                        p.a = 255;
                    }
                }
            }
        }

        // Owned frame handle: `buf.clone()` bumps the reference count — the pixels are
        // NOT copied. The host adapter (slint `from_rgba8` / Qt `as_raw`) shares this
        // exact storage.
        PaneSurface::Cpu(CpuSurface { buf: buf.clone() })
    }
}

#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "gpu")]
pub use gpu::{atlas_bytes, bg_buffer_bytes, GpuRenderer, DEFAULT_BG_CAP};
