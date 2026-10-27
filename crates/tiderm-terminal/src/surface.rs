//! UI-framework-neutral frame surfaces for the Qt host and headless tests.
//!
//! [`PaneRenderer`](crate::render::PaneRenderer) returns an owned [`PaneSurface`].
//! [`PixelBuf`] uses an `Arc`-based COW buffer and [`Rgba8`] = `rgb::RGBA8`.
//! COW is load-bearing: the renderer flips between two buffers and writes into the
//!   one the UI is not currently displaying, relying on `make_mut` copying only when
//!   the previous frame is still shared.
//!
//! `clone()` on any of these types bumps a reference count — never a pixel copy.

use std::sync::Arc;

/// The frame pixel type on every storage path.
pub type Rgba8 = rgb::RGBA8;

/// Dims + pixels for the COW buffer. Fields are crate-internal —
/// construct via [`PixelBuf::alloc`].
#[derive(Clone)]
pub struct NeutralBuf {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) px: Vec<Rgba8>,
}

/// Framework-neutral frame pixel storage.
#[derive(Clone)]
pub struct PixelBuf(Arc<NeutralBuf>);

impl PixelBuf {
    /// Allocate a zeroed `w×h` frame.
    pub fn alloc(w: u32, h: u32) -> Self {
        let len = w as usize * h as usize;
        Self(Arc::new(NeutralBuf {
            w,
            h,
            px: vec![Rgba8::default(); len],
        }))
    }

    /// Writable pixels for this frame. COW: copies only when the buffer is still
    /// shared with a previously-published frame — the double-buffer contract the
    /// software renderer relies on (`self.idx ^= 1`).
    pub fn make_mut_rgba(&mut self) -> &mut [Rgba8] {
        Arc::make_mut(&mut self.0).px.as_mut_slice()
    }

    /// Read-only pixel view.
    pub fn as_rgba(&self) -> &[Rgba8] {
        self.0.px.as_slice()
    }

    /// Raw byte view (`width*4` per row), via rgb's own trait — no unsafe.
    pub fn as_rgba_bytes(&self) -> &[u8] {
        use rgb::ComponentBytes;
        self.as_rgba().as_bytes()
    }

    pub fn width(&self) -> u32 {
        self.0.w
    }

    pub fn height(&self) -> u32 {
        self.0.h
    }

    /// Diagnostic: does the next `make_mut_rgba()` write in place?
    pub fn is_unique(&self) -> bool {
        Arc::strong_count(&self.0) == 1
    }
}

/// One CPU frame: pixels + dims ([`PixelBuf`] carries both).
#[derive(Clone)]
pub struct CpuSurface {
    pub buf: PixelBuf,
}

impl CpuSurface {
    /// Read-only pixel view.
    pub fn as_rgba(&self) -> &[Rgba8] {
        self.buf.as_rgba()
    }

    /// Raw byte view (`width*4` per row).
    pub fn as_rgba_bytes(&self) -> &[u8] {
        self.buf.as_rgba_bytes()
    }

    pub fn width(&self) -> u32 {
        self.buf.width()
    }

    pub fn height(&self) -> u32 {
        self.buf.height()
    }

    /// Bytes per row — tightly packed (`width*4`), matching the pre-Phase-A
    /// `SharedPixelBuffer` layout the renderer has always produced.
    pub fn stride_bytes(&self) -> u32 {
        self.width() * 4
    }

    /// FFI view of the frame. `ptr` stays valid while `self` is alive and no
    /// `make_mut_rgba()` runs in between — a Qt host consumes it synchronously inside
    /// its paint handler, which satisfies both. The cxx bridge carries `ptr` as
    /// `usize` (cxx rejects raw pointers; ADR-011 §2.2 constraint 1/2).
    pub fn as_raw(&self) -> RawSurface {
        RawSurface {
            ptr: self.as_rgba_bytes().as_ptr(),
            w: self.width(),
            h: self.height(),
            stride: self.stride_bytes(),
        }
    }
}

/// POD twin of the planned cxx-bridge `Frame` struct (`ptr`/`w`/`h`/`stride`).
#[repr(C)]
pub struct RawSurface {
    pub ptr: *const u8,
    pub w: u32,
    pub h: u32,
    pub stride: u32,
}

/// GPU frame: the freshly rendered `wgpu::Texture` (there is no CPU pointer on this
/// path — hence [`PaneSurface`] being an enum). Qt would need QRhi interop, which is
/// currently deferred; the Qt host uses text spans and the CPU probe path.
#[cfg(feature = "gpu")]
#[derive(Clone)]
pub struct GpuSurface {
    pub texture: wgpu::Texture,
    pub w: u32,
    pub h: u32,
}

/// A produced frame — owned, storable across frames, and UI-framework-neutral.
#[derive(Clone)]
pub enum PaneSurface {
    Cpu(CpuSurface),
    #[cfg(feature = "gpu")]
    Gpu(GpuSurface),
}

impl PaneSurface {
    /// The CPU frame, when this is one (CPU-path hosts / examples / PNG proofs).
    pub fn as_cpu(&self) -> Option<&CpuSurface> {
        match self {
            PaneSurface::Cpu(c) => Some(c),
            #[cfg(feature = "gpu")]
            PaneSurface::Gpu(_) => None,
        }
    }

    pub fn width(&self) -> u32 {
        match self {
            PaneSurface::Cpu(c) => c.width(),
            #[cfg(feature = "gpu")]
            PaneSurface::Gpu(g) => g.w,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            PaneSurface::Cpu(c) => c.height(),
            #[cfg(feature = "gpu")]
            PaneSurface::Gpu(g) => g.h,
        }
    }
}

impl Default for PaneSurface {
    /// A 1×1 blank frame — the startup placeholder before the first render.
    fn default() -> Self {
        PaneSurface::Cpu(CpuSurface {
            buf: PixelBuf::alloc(1, 1),
        })
    }
}
