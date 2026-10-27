mod atlas;
mod frame;
use super::*;

// =================================================================================
// GPU renderer
// =================================================================================

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _pad: [f32; 2],
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BgInstance {
    rect: [f32; 4],
    color: [f32; 4],
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
}

#[cfg(feature = "gpu")]
const ATLAS: u32 = 1024;

/// Initial background-instance buffer capacity (cells). Grows only by `next_power_of_two`
/// past this; 1024 since P1 (was 4096) — ~96 KB smaller initial GPU allocation per pane.
#[cfg(feature = "gpu")]
pub const DEFAULT_BG_CAP: u64 = 1024;

/// Per-pane atlas texture footprint in bytes (R8 coverage, 1 byte/texel). 1024² = 1 MB since
/// P1 (was 4 MB at 2048²). Reported by the memory-budget harness.
#[cfg(feature = "gpu")]
#[inline]
pub const fn atlas_bytes() -> usize {
    (ATLAS as usize) * (ATLAS as usize)
}

/// Per-pane background-instance buffer footprint in bytes (initial allocation; grows only
/// past 1024 cells). Reported by the memory-budget harness.
#[cfg(feature = "gpu")]
#[inline]
pub const fn bg_buffer_bytes() -> usize {
    (DEFAULT_BG_CAP as usize) * std::mem::size_of::<BgInstance>()
}

#[cfg(feature = "gpu")]
struct AtlasEntry {
    uv: [f32; 4], // normalized x,y,w,h
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

#[cfg(feature = "gpu")]
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,

    atlas_tex: wgpu::Texture,
    atlas_alloc: etagere::AtlasAllocator,
    atlas_map: std::collections::HashMap<GlyphKey, AtlasEntry>,
    /// Generation of the `Font` the atlas was packed from. `GlyphKey` is keyed only by
    /// (font_id, gid, …), so on a font/size reload the gids/metrics change but the key can
    /// collide — guard against stale-glyph bleed + leak by clearing the atlas when this no
    /// longer matches `Font::generation()`. `0` = nothing packed yet.
    atlas_gen: u64,
    /// Set false at the start of each `render_to_texture`; flipped true after a one-time
    /// atlas reset on allocation failure, so a pathological CJK-heavy screen can't loop
    /// resetting the packer per glyph (it drops the offending glyph instead).
    atlas_reset_this_frame: bool,

    uniform_buf: wgpu::Buffer,
    bg_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    bg_bind: wgpu::BindGroup,
    glyph_bind: wgpu::BindGroup,

    bg_buf: wgpu::Buffer,
    bg_cap: u64,
    glyph_buf: wgpu::Buffer,
    glyph_cap: u64,

    target: Option<wgpu::Texture>,
    tw: u32,
    th: u32,
    /// Last frame handed out. Phase A: the Slint texture import moved host-side
    /// (`slint_adapter::to_image`), so the renderer no longer keeps the imported
    /// `Image` itself — hosts that must survive a transient import failure can
    /// re-present this (see [`GpuRenderer::last_surface`]).
    last_surface: Option<PaneSurface>,
}

#[cfg(feature = "gpu")]
impl GpuRenderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // bind layout 0: uniform only (bg)
        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // bind layout 1: uniform + atlas + sampler (glyph)
        let glyph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let bg_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind"),
            layout: &bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let glyph_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph-bind"),
            layout: &glyph_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("term-shaders"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let color_target = wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba8Unorm,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let bg_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg-pl"),
            bind_group_layouts: &[Some(&bg_layout)],
            immediate_size: 0,
        });
        let glyph_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph-pl"),
            bind_group_layouts: &[Some(&glyph_layout)],
            immediate_size: 0,
        });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg-pipe"),
            layout: Some(&bg_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                targets: &[Some(color_target.clone())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph-pipe"),
            layout: Some(&glyph_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_glyph"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_glyph"),
                targets: &[Some(color_target)],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let bg_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg-instances"),
            size: 1024 * std::mem::size_of::<BgInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyph_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph-instances"),
            size: 8192 * std::mem::size_of::<GlyphInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        GpuRenderer {
            device,
            queue,
            atlas_tex,
            atlas_alloc: etagere::AtlasAllocator::new(etagere::size2(ATLAS as i32, ATLAS as i32)),
            atlas_map: std::collections::HashMap::new(),
            atlas_gen: 0,
            atlas_reset_this_frame: false,
            uniform_buf,
            bg_pipeline,
            glyph_pipeline,
            bg_bind,
            glyph_bind,
            bg_buf,
            bg_cap: 1024,
            glyph_buf,
            glyph_cap: 8192,
            target: None,
            tw: 0,
            th: 0,
            last_surface: None,
        }
    }
}

#[cfg(feature = "gpu")]
#[cfg(feature = "gpu")]
#[cfg(feature = "gpu")]
impl PaneRenderer for GpuRenderer {
    fn name(&self) -> &'static str {
        "gpu (swash atlas → wgpu texture)"
    }

    fn render(&mut self, grid: &GridSnapshot, font: &mut Font, opts: &RenderOpts) -> PaneSurface {
        self.render_to_texture(grid, font, opts);
        // Hand the freshly-rendered texture out as an owned GpuSurface (a texture
        // handle clone = refcount bump, not a copy). The Slint import
        // (`Image::try_from` — shared device → zero copy) moved host-side to
        // `slint_adapter::to_image`; a transient failure there (device-lost / RDP
        // transition) now falls back to a blank image instead of the last good frame —
        // the one documented Phase A behavior delta. Hosts needing the old fallback
        // can re-present [`GpuRenderer::last_surface`].
        let sfc = PaneSurface::Gpu(GpuSurface {
            texture: self.target.as_ref().unwrap().clone(),
            w: self.tw,
            h: self.th,
        });
        self.last_surface = Some(sfc.clone());
        sfc
    }
}

#[cfg(feature = "gpu")]
const SHADER: &str = r#"
struct U { screen: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;

fn quad_corner(vi: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    return c[vi];
}
fn to_ndc(px: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(px.x / u.screen.x * 2.0 - 1.0, 1.0 - px.y / u.screen.y * 2.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

struct BgOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs_bg(@builtin(vertex_index) vi: u32,
                 @location(0) rect: vec4<f32>,
                 @location(1) color: vec4<f32>) -> BgOut {
    let c = quad_corner(vi);
    let px = rect.xy + c * rect.zw;
    var o: BgOut;
    o.pos = to_ndc(px);
    o.color = color;
    return o;
}
@fragment fn fs_bg(i: BgOut) -> @location(0) vec4<f32> { return i.color; }

struct GlyphOut { @builtin(position) pos: vec4<f32>,
                  @location(0) uv: vec2<f32>,
                  @location(1) color: vec4<f32> };
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@vertex fn vs_glyph(@builtin(vertex_index) vi: u32,
                    @location(0) rect: vec4<f32>,
                    @location(1) uvr: vec4<f32>,
                    @location(2) color: vec4<f32>) -> GlyphOut {
    let c = quad_corner(vi);
    let px = rect.xy + c * rect.zw;
    var o: GlyphOut;
    o.pos = to_ndc(px);
    o.uv = uvr.xy + c * uvr.zw;
    o.color = color;
    return o;
}
@fragment fn fs_glyph(i: GlyphOut) -> @location(0) vec4<f32> {
    let cov = textureSample(atlas, samp, i.uv).r;
    return vec4<f32>(i.color.rgb, i.color.a * cov);
}
"#;
