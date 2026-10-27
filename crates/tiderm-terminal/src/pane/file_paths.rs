//! TerminalPane: file paths.
use super::*;

impl TerminalPane {
    // ---- Clickable file paths --------------------------------------------------------------
    //
    // Plain click opens the file (editor / OS default); Ctrl/Cmd-click copies the resolved
    // absolute path. Paths are verified on disk (against this pane's cwd) before they linkify,
    // so prose tokens don't light up. The grid extraction lives in [`crate::links`]; resolve +
    // resolution lives in the local file-links module; activation belongs to the host. This is the renderer-side glue (Spike's
    // `Terminal.tsx` link provider, ported to the cell grid).

    /// Set this pane's working directory (the base for resolving relative path tokens). Clearing
    /// or changing it drops the verify cache, since the same token can resolve elsewhere.
    pub fn set_cwd(&mut self, cwd: Option<String>) {
        if cwd != self.cwd {
            self.cwd = cwd;
            self.verified.clear();
        }
    }

    /// The logical-px cell size for a surface of `surf_w`×`surf_h` (the terminal image is
    /// stretched `image-fit: fill` over the pane body), or `None` for a degenerate pane.
    pub(super) fn cell_logical(
        &self,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<(f32, f32, usize, usize)> {
        let (cols, rows) = self.grid_size();
        if cols == 0 || rows == 0 || surf_w <= 0.0 || surf_h <= 0.0 {
            return None;
        }
        // With the gutter on, the body width covers `cols + gutter` cells (the host
        // derives grid cols from `body − gutter`); the text cell width divides that.
        Some((
            surf_w / (cols + self.gutter_cells) as f32,
            surf_h / rows as f32,
            cols,
            rows,
        ))
    }

    /// Reconstruct one viewport row's text (one char per column; blanks as spaces) so the
    /// `links` extractor's column indices line up with cells. Exact for ASCII paths.
    ///
    /// CJK: a wide glyph occupies 2 columns — its right half is a `wide_spacer` cell whose
    /// `ch` is a space. Skipping it keeps "潮汐" as "潮汐" instead of "潮 汐" in extracted
    /// text (screen_text / AI context / structured-output scanning). Rendering is unaffected
    /// (the renderer already skips wide_spacer via the `wide`/`wide_spacer` flags).
    pub(super) fn row_text(snap: &crate::grid::GridSnapshot, row: usize) -> String {
        (0..snap.cols)
            .filter_map(|col| {
                let cell = snap.cell(col, row);
                if cell.wide_spacer {
                    return None; // right half of a wide CJK glyph — no glyph to emit
                }
                let ch = cell.ch;
                Some(if ch == '\0' { ' ' } else { ch })
            })
            .collect()
    }

    pub(super) fn cache_key(&self, token: &str) -> String {
        format!("{}\u{1f}{}", self.cwd.as_deref().unwrap_or(""), token)
    }

    /// Find a verified path token under the (logical-px) point, returning the resolved record,
    /// the candidate's column span, and the cell metrics. Resolution is cached per (cwd, token);
    /// only existing paths are cached, so freshly-created files linkify on a later hover.
    pub(super) fn locate(
        &mut self,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<(LocalTarget, usize, usize, usize, f32, f32)> {
        let (cell_w, cell_h, cols, rows) = self.cell_logical(surf_w, surf_h)?;
        let x = x - self.gutter_cells as f32 * cell_w;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let col = (x / cell_w) as usize;
        let row = (y / cell_h) as usize;
        if col >= cols || row >= rows {
            return None;
        }

        let snap = self.grid.snapshot();
        let text = Self::row_text(&snap, row);
        let cand = extract_path_candidates(&text)
            .into_iter()
            .find(|c| col >= c.start && col < c.end)?;

        let key = self.cache_key(&cand.path);
        let resolved = if let Some(hit) = self.verified.get(&key) {
            hit.clone()
        } else {
            let r = file_links::resolve(self.cwd.as_deref(), &cand.path)?;
            if self.verified.len() >= 512 {
                self.verified.clear();
            }
            self.verified.insert(key, r.clone());
            r
        };
        Some((resolved, cand.start, cand.end, row, cell_w, cell_h))
    }

    /// Find an http/https URL under the (logical-px) point, returning the candidate, its row,
    /// and the cell metrics. URLs linkify on shape alone — no disk/network verification (so no
    /// cache either; extraction per hover is cheap, same as the path re-extract in `link_at`).
    pub(super) fn url_under(
        &self,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<(UrlCandidate, usize, f32, f32)> {
        let (cell_w, cell_h, cols, rows) = self.cell_logical(surf_w, surf_h)?;
        let x = x - self.gutter_cells as f32 * cell_w;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let col = (x / cell_w) as usize;
        let row = (y / cell_h) as usize;
        if col >= cols || row >= rows {
            return None;
        }
        let snap = self.grid.snapshot();
        let text = Self::row_text(&snap, row);
        let cand = extract_url_candidates(&text)
            .into_iter()
            .find(|c| col >= c.start && col < c.end)?;
        Some((cand, row, cell_w, cell_h))
    }

    /// Hit-test a (logical-px) hover point against the rendered grid. Returns the underline rect +
    /// target when the point is over an http/https URL or a path that exists on disk, else `None`.
    /// The candidate's `:line[:col]` is carried through (and shown in the tooltip), but only the
    /// resolved path is verified — mirroring the Electron link provider. URLs win when a token is
    /// both (a URL is path-shaped but never disk-verifies anyway).
    pub fn link_at(&mut self, x: f32, y: f32, surf_w: f32, surf_h: f32) -> Option<LinkHit> {
        if let Some(hit) = self.url_link_at(x, y, surf_w, surf_h) {
            return Some(hit);
        }
        let (r, start, end, row, cell_w, cell_h) = self.locate(x, y, surf_w, surf_h)?;
        // Recover the candidate's line/col by re-extracting (cheap; same row). The resolved
        // record only carries the path, so pull location off the candidate that covered the cell.
        let snap = self.grid.snapshot();
        let text = Self::row_text(&snap, row);
        let cand = extract_path_candidates(&text)
            .into_iter()
            .find(|c| c.start == start && c.end == end);
        let (line, col) = cand
            .as_ref()
            .map(|c| (c.line, c.col))
            .unwrap_or((None, None));

        let tip = match (line, col) {
            (Some(l), Some(c)) => format!("{}:{}:{}", r.abs_path, l, c),
            (Some(l), None) => format!("{}:{}", r.abs_path, l),
            _ => r.abs_path.clone(),
        };
        Some(LinkHit {
            x: start as f32 * cell_w,
            y: (row as f32 + 1.0) * cell_h - 1.0, // a hairline along the cell's baseline
            w: (end - start) as f32 * cell_w,
            abs_path: r.abs_path,
            line,
            col,
            tip,
            is_url: false,
        })
    }
}
