use super::*;
impl GpuRenderer {
    pub(super) fn ensure_glyph(&mut self, font: &mut Font, key: GlyphKey) -> bool {
        if self.atlas_map.contains_key(&key) {
            return true;
        }
        let g = font.rasterize(key).clone();
        if g.w == 0 || g.h == 0 {
            self.atlas_map.insert(
                key,
                AtlasEntry {
                    uv: [0.0; 4],
                    w: 0,
                    h: 0,
                    left: 0,
                    top: 0,
                },
            );
            return true;
        }
        let pad = 1;
        let alloc = match self
            .atlas_alloc
            .allocate(etagere::size2((g.w + pad) as i32, (g.h + pad) as i32))
        {
            Some(a) => a,
            None => {
                // Atlas full (only on pathological CJK-heavy screens at 1024²). Reset the
                // packer once per frame and retry; if still full, drop the glyph rather than
                // spin. Already-packed glyphs keep their atlas copy, so this only costs a
                // one-frame re-pack of the visible set.
                if !self.atlas_reset_this_frame {
                    self.atlas_alloc.clear();
                    self.atlas_map.clear();
                    self.atlas_reset_this_frame = true;
                    match self
                        .atlas_alloc
                        .allocate(etagere::size2((g.w + pad) as i32, (g.h + pad) as i32))
                    {
                        Some(a) => a,
                        None => return false,
                    }
                } else {
                    return false;
                }
            }
        };
        let x = alloc.rectangle.min.x as u32;
        let y = alloc.rectangle.min.y as u32;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &g.mask,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(g.w),
                rows_per_image: Some(g.h),
            },
            wgpu::Extent3d {
                width: g.w,
                height: g.h,
                depth_or_array_layers: 1,
            },
        );
        self.atlas_map.insert(
            key,
            AtlasEntry {
                uv: [
                    x as f32 / ATLAS as f32,
                    y as f32 / ATLAS as f32,
                    g.w as f32 / ATLAS as f32,
                    g.h as f32 / ATLAS as f32,
                ],
                w: g.w,
                h: g.h,
                left: g.left,
                top: g.top,
            },
        );
        true
    }
}
