//! Source-row projection shared by raster and semantic rendering.
use super::*;

impl TermGrid {
    /// Fill `cells` (len `cols*rows`) with the viewport at the current virtual offset.
    ///
    /// `source_row(r)` maps the viewport through the fold projection to `abs`:
    ///   * `abs >= -history` → alacritty's raw window (`grid[Line(abs)]`);
    ///   * `abs < -history` → the compressed [`ScrollbackStore`] (row
    ///     `store.rows + history + abs`); older than that → default (blank) cells.
    ///
    /// The cursor is only visible when the viewport is inside alacritty's window.
    fn render_viewport_into(&self, cells: &mut [RenderCell]) -> ((usize, usize), bool) {
        let cols = self.size.cols;
        let rows = self.size.rows;
        let default_fg = [
            self.palette[7].r,
            self.palette[7].g,
            self.palette[7].b,
            0xff,
        ];
        let default_bg = [
            self.palette[0].r,
            self.palette[0].g,
            self.palette[0].b,
            0xff,
        ];
        let h = self.term.grid().history_size();
        let s = self.stored_rows();
        let grid = self.term.grid();

        for r in 0..rows {
            let base = r * cols;
            let Some(abs) = self.source_row(r) else {
                cells[base..base + cols].fill(RenderCell::default());
                continue;
            };
            let abs = i64::from(abs);
            if let Some(count) = self.fold_marker(abs as i32) {
                cells[base..base + cols].fill(RenderCell::default());
                for (col, ch) in format!("▶ … ({count})").chars().take(cols).enumerate() {
                    cells[base + col] = RenderCell {
                        ch,
                        fg: default_fg,
                        bg: default_bg,
                        bold: true,
                        ..RenderCell::default()
                    };
                }
                continue;
            }
            if abs >= -(h as i64) {
                // Inside alacritty's raw window (Line(abs) valid for abs in [-h, rows-1]).
                let row = &grid[Line(abs as i32)];
                for c in 0..cols {
                    cells[base + c] = self.render_cell(&row[Column(c)]);
                }
            } else if abs >= -((s + h) as i64) {
                // Inside the compressed store.
                let idx = (s as i64 + h as i64 + abs) as usize;
                let mut cache = self.row_cache.borrow_mut();
                if let Some(pos) = cache.iter().position(|(i, _)| *i == idx) {
                    let (_, rc) = &cache[pos];
                    cells[base..base + cols].copy_from_slice(rc);
                    let entry = cache.remove(pos);
                    cache.push(entry);
                } else if let Some(ld) = self.store.row(idx) {
                    let rc = render_cells_from_line_data(&ld, cols);
                    cells[base..base + cols].copy_from_slice(&rc);
                    if cache.len() >= ROW_CACHE_MAX {
                        cache.remove(0);
                    }
                    cache.push((idx, rc));
                } else {
                    cells[base..base + cols].fill(RenderCell::default());
                }
            } else {
                cells[base..base + cols].fill(RenderCell::default());
            }
        }

        let content = self.term.renderable_content();
        let point = content.cursor.point;
        let row = self.screen_row(point.line.0);
        let cursor_visible = row.is_some() && point.column.0 < cols;
        let cursor = row.map_or((0, 0), |row| (point.column.0, row));
        (cursor, cursor_visible)
    }

    /// Produce a flat, resolved-RGBA snapshot of the current viewport for a renderer.
    pub fn snapshot(&self) -> GridSnapshot {
        let cols = self.size.cols;
        let rows = self.size.rows;
        let mut cells = vec![RenderCell::default(); cols * rows];
        let (cursor, cursor_visible) = self.render_viewport_into(&mut cells);
        let (buffer_lines, row_numbers) = self.gutter_numbers();
        GridSnapshot {
            cols,
            rows,
            cells,
            cursor,
            cursor_visible,
            default_bg: [
                self.palette[0].r,
                self.palette[0].g,
                self.palette[0].b,
                0xff,
            ],
            default_fg: [
                self.palette[7].r,
                self.palette[7].g,
                self.palette[7].b,
                0xff,
            ],
            buffer_lines,
            row_numbers,
        }
    }

    /// Like [`snapshot`](Self::snapshot), but reuses a caller-provided cell buffer to
    /// avoid reallocating the viewport Vec on every render.
    pub fn snapshot_into(&self, cells: &mut Vec<RenderCell>) -> GridSnapshot {
        let cols = self.size.cols;
        let rows = self.size.rows;
        // Every projected row is overwritten above, including blanks and fold
        // markers. Preserve the existing cells instead of filling the viewport
        // with defaults only to overwrite it again on every frame.
        cells.resize(cols * rows, RenderCell::default());
        let (cursor, cursor_visible) = self.render_viewport_into(cells);
        let (buffer_lines, row_numbers) = self.gutter_numbers();
        GridSnapshot {
            cols,
            rows,
            cells: std::mem::take(cells),
            cursor,
            cursor_visible,
            default_bg: [
                self.palette[0].r,
                self.palette[0].g,
                self.palette[0].b,
                0xff,
            ],
            default_fg: [
                self.palette[7].r,
                self.palette[7].g,
                self.palette[7].b,
                0xff,
            ],
            buffer_lines,
            row_numbers,
        }
    }

    /// `(buffer_lines, row_numbers)` for the line-number gutter: the retained buffer
    /// height and the 1-based buffer line each viewport row shows.
    fn gutter_numbers(&self) -> (u64, Vec<u64>) {
        let s = self.stored_rows() as i64;
        let h = self.term.grid().history_size() as i64;
        let buffer_lines = (s + h + self.size.rows as i64).max(1) as u64;
        let row_numbers = (0..self.size.rows)
            .map(|r| {
                self.source_row(r)
                    .map_or(0, |line| (i64::from(line) + s + h + 1).max(1) as u64)
            })
            .collect();
        (buffer_lines, row_numbers)
    }
}
