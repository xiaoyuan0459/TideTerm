//! TerminalPane: drawing.
use super::*;

impl TerminalPane {
    /// Render the current grid to an owned
    /// [`PaneSurface`](crate::surface::PaneSurface) at the pane's *physical* pixel
    /// resolution (`cols*cell_w × rows*cell_h`). Cheap to call repeatedly — the renderer
    /// caches its buffers/atlas — but gate it on [`take_dirty`](Self::take_dirty) plus the
    /// cursor blink for minimal CPU.
    pub fn render(&mut self, font: &mut Font, opts: &RenderOpts) -> PaneSurface {
        let mut snap = self.grid.snapshot_into(&mut self.snapshot_cells);
        self.reading_snapshot(&mut snap);
        self.gutter_cells = if opts.gutter {
            crate::render::gutter_cells(snap.buffer_lines)
        } else {
            0
        };
        let surface = self.renderer.render(&snap, font, opts);
        // Keep the backing allocation for reuse on the next frame.
        self.snapshot_cells = std::mem::take(&mut snap.cells);
        surface
    }

    /// 采集当前可见网格为**无像素**绘制记录流（宿主用文本栈自己画字形；见 [`crate::spans`]）。
    /// 与 [`render`](Self::render) 走同一套快照与 pass 顺序，但不落任何帧缓冲：
    /// `font` 只读度量（`&Font`），字形光栅化完全交给宿主。
    pub fn spans(&mut self, font: &Font, opts: &RenderOpts) -> Vec<u32> {
        let mut snap = self.grid.snapshot_into(&mut self.snapshot_cells);
        self.reading_snapshot(&mut snap);
        self.gutter_cells = if opts.gutter {
            crate::render::gutter_cells(snap.buffer_lines)
        } else {
            0
        };
        let m = crate::spans::Metrics {
            cell_w: font.cell_w,
            cell_h: font.cell_h,
            ascent: font.ascent,
        };
        let out = crate::spans::collect(&snap, &m, opts);
        self.snapshot_cells = std::mem::take(&mut snap.cells);
        out
    }

    /// A human-readable name for the active renderer (e.g. for a HUD/debug overlay).
    pub fn renderer_name(&self) -> &'static str {
        self.renderer.name()
    }

    /// Apply a colour theme: override the 16 base ANSI colours (index 0 = default background,
    /// 7 = default foreground). See [`crate::grid::TermGrid::set_base16`].
    pub fn set_palette(&mut self, base: [[u8; 3]; 16]) {
        self.grid.set_base16(base);
    }

    /// Swap the renderer at runtime (e.g. GPU↔software on a device-lost / RDP transition).
    /// The next [`render`](Self::render) rebuilds from the live grid, so the swap is seamless.
    pub fn set_renderer(&mut self, renderer: Box<dyn PaneRenderer>) {
        self.renderer = renderer;
    }
}
