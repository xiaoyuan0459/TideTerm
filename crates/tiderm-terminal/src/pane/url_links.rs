//! URL-only hit testing never consults the local filesystem.
use super::{LinkHit, TerminalPane};

impl TerminalPane {
    pub fn url_link_at(&self, x: f32, y: f32, surf_w: f32, surf_h: f32) -> Option<LinkHit> {
        if let Some((cand, row, cell_w, cell_h)) = self.url_under(x, y, surf_w, surf_h) {
            return Some(LinkHit {
                x: cand.start as f32 * cell_w,
                y: (row as f32 + 1.0) * cell_h - 1.0, // a hairline along the cell's baseline
                w: (cand.end - cand.start) as f32 * cell_w,
                tip: cand.url.clone(),
                abs_path: cand.url,
                line: None,
                col: None,
                is_url: true,
            });
        }
        None
    }
}
