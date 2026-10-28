use super::*;
impl GpuRenderer {
    /// Do the GPU work (build instances, upload, draw into the per-pane target, submit).
    /// Separated from the Slint import so the benchmark can time pure render throughput.
    pub fn render_to_texture(&mut self, grid: &GridSnapshot, font: &mut Font, opts: &RenderOpts) {
        // Font/size reload? The glyph keys carry no font epoch, so old atlas entries would
        // bleed stale glyphs (and leak until "atlas full"). Drop them and start the packer
        // over against the new font.
        if font.generation() != self.atlas_gen {
            self.atlas_map.clear();
            self.atlas_alloc.clear();
            self.atlas_gen = font.generation();
        }

        self.atlas_reset_this_frame = false;
        let cw = font.cell_w;
        let ch = font.cell_h;
        let gw = if opts.gutter {
            gutter_cells(grid.buffer_lines) as u32 * cw
        } else {
            0
        };
        let w = (grid.cols as u32 * cw + gw).max(1);
        let h = (grid.rows as u32 * ch).max(1);

        if self.target.is_none() || w != self.tw || h != self.th {
            self.tw = w;
            self.th = h;
            self.target = Some(self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pane-target"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }));
        }

        // Build instance arrays.
        let mut bgs: Vec<BgInstance> = Vec::with_capacity(grid.cols * grid.rows / 4);
        let mut glyphs: Vec<GlyphInstance> = Vec::with_capacity(grid.cols * grid.rows);
        let to_f = |c: [u8; 4]| {
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                c[3] as f32 / 255.0,
            ]
        };

        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let cell = grid.cell(col, row);
                if cell.wide_spacer {
                    continue;
                }
                let x0 = (gw + col as u32 * cw) as f32;
                let y0 = (row as u32 * ch) as f32;
                let cell_w = if cell.wide {
                    (cw * 2) as f32
                } else {
                    cw as f32
                };

                if cell.bg[3] > 0 {
                    bgs.push(BgInstance {
                        rect: [x0, y0, cell_w, ch as f32],
                        color: to_f(cell.bg),
                    });
                }

                if cell.ch != ' ' && cell.ch != '\0' {
                    let (font_id, gid) = font.resolve(cell.ch);
                    let key = GlyphKey {
                        font_id,
                        gid,
                        bold: cell.bold,
                        italic: cell.italic,
                    };
                    self.ensure_glyph(font, key);
                    if let Some(e) = self.atlas_map.get(&key) {
                        if e.w > 0 {
                            // Center a wide (CJK) glyph in its 2-cell span (T1.5 layout).
                            let gx =
                                crate::layout::pen_x(col as u32 * cw, cw, cell.wide, e.w, e.left)
                                    as f32
                                    + e.left as f32;
                            let gy = y0 + font.ascent as f32 - e.top as f32;
                            glyphs.push(GlyphInstance {
                                rect: [gx, gy, e.w as f32, e.h as f32],
                                uv: e.uv,
                                color: to_f(cell.fg),
                            });
                        }
                    }
                }

                if cell.underline {
                    // Clamp the underline within this cell's band (ascent is i32 → f32, so no
                    // u32 wrap), keeping it off the next row / bottom of the target.
                    let uy = (y0 + font.ascent as f32 + 2.0).clamp(y0, y0 + ch as f32 - 1.0);
                    bgs.push(BgInstance {
                        rect: [x0, uy, cell_w, 1.0],
                        color: to_f(cell.fg),
                    });
                }
            }
        }

        // Line-number gutter numbers (dim fg), as glyph instances left of the text.
        if gw > 0 {
            let dim = to_f(gutter_color(grid.default_fg, grid.default_bg));
            let gcells = (gw / cw) as usize;
            let width = gcells.saturating_sub(1);
            for (row, &n) in grid
                .row_numbers
                .iter()
                .enumerate()
                .filter(|(_, n)| **n != 0)
            {
                let text = format!("{:>width$}", n, width = width);
                let y0 = (row as u32 * ch) as f32;
                for (di, dch) in text.chars().enumerate() {
                    if dch == ' ' {
                        continue;
                    }
                    let (font_id, gid) = font.resolve(dch);
                    let key = GlyphKey {
                        font_id,
                        gid,
                        bold: false,
                        italic: false,
                    };
                    self.ensure_glyph(font, key);
                    if let Some(e) = self.atlas_map.get(&key) {
                        if e.w > 0 {
                            let x0 = (di as u32 * cw) as f32 + e.left as f32;
                            let gy = y0 + font.ascent as f32 - e.top as f32;
                            glyphs.push(GlyphInstance {
                                rect: [x0, gy, e.w as f32, e.h as f32],
                                uv: e.uv,
                                color: dim,
                            });
                        }
                    }
                }
            }
        }

        // Cursor quad: block = full inverted cell; underline/bar = thin fg band
        // (WindTerm-style outline cursors).
        if grid.cursor_visible && opts.cursor_on {
            let (col, row) = grid.cursor;
            let cell = grid.cell(col.min(grid.cols - 1), row.min(grid.rows - 1));
            let block_w = if cell.wide { cw * 2 } else { cw };
            let (cx, cy, cwid, chei) = match opts.cursor_style {
                1 => {
                    let uh = (ch / 5).max(2) as f32;
                    (
                        gw as f32 + (col as u32 * cw) as f32,
                        (row as u32 * ch) as f32 + ch as f32 - uh,
                        block_w as f32,
                        uh,
                    )
                }
                2 => {
                    let bw = (cw / 5).max(2) as f32;
                    (
                        gw as f32 + (col as u32 * cw) as f32,
                        (row as u32 * ch) as f32,
                        bw,
                        ch as f32,
                    )
                }
                _ => (
                    gw as f32 + (col as u32 * cw) as f32,
                    (row as u32 * ch) as f32,
                    block_w as f32,
                    ch as f32,
                ),
            };
            bgs.push(BgInstance {
                rect: [cx, cy, cwid, chei],
                color: to_f(cell.fg),
            });
        }

        // Upload uniforms + instances (grow buffers if needed).
        self.queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                screen: [w as f32, h as f32],
                _pad: [0.0; 2],
            }),
        );
        if bgs.len() as u64 > self.bg_cap {
            self.bg_cap = (bgs.len() as u64).next_power_of_two();
            self.bg_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg-instances"),
                size: self.bg_cap * std::mem::size_of::<BgInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if glyphs.len() as u64 > self.glyph_cap {
            self.glyph_cap = (glyphs.len() as u64).next_power_of_two();
            self.glyph_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glyph-instances"),
                size: self.glyph_cap * std::mem::size_of::<GlyphInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bgs.is_empty() {
            self.queue
                .write_buffer(&self.bg_buf, 0, bytemuck::cast_slice(&bgs));
        }
        if !glyphs.is_empty() {
            self.queue
                .write_buffer(&self.glyph_buf, 0, bytemuck::cast_slice(&glyphs));
        }

        let target = self.target.as_ref().unwrap();
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let bg0 = grid.default_bg;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pane-encoder"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pane-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg0[0] as f64 / 255.0,
                            g: bg0[1] as f64 / 255.0,
                            b: bg0[2] as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !bgs.is_empty() {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_bind_group(0, &self.bg_bind, &[]);
                pass.set_vertex_buffer(0, self.bg_buf.slice(..));
                pass.draw(0..6, 0..bgs.len() as u32);
            }
            if !glyphs.is_empty() {
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &self.glyph_bind, &[]);
                pass.set_vertex_buffer(0, self.glyph_buf.slice(..));
                pass.draw(0..6, 0..glyphs.len() as u32);
            }
        }
        self.queue.submit(Some(enc.finish()));
    }

    /// Block until the most recent submission completes (benchmark timing only).
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// The last frame handed out — hosts may re-present it without re-rendering
    /// (e.g. to ride out a transient Slint texture-import failure).
    pub fn last_surface(&self) -> Option<&PaneSurface> {
        self.last_surface.as_ref()
    }
}
