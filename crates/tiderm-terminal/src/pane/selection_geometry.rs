//! Pointer and selection geometry use the same visible/source row projection.
use super::*;

impl TerminalPane {
    pub(super) fn cell_at_clamped(
        &self,
        x: f32,
        y: f32,
        surf_w: f32,
        surf_h: f32,
    ) -> Option<selection::Cell> {
        let (cell_w, cell_h, cols, rows) = self.cell_logical(surf_w, surf_h)?;
        let x = x - self.gutter_cells as f32 * cell_w;
        let col = (x / cell_w).floor().clamp(0.0, (cols - 1) as f32) as usize;
        let row = (y / cell_h).floor().clamp(0.0, (rows - 1) as f32) as usize;
        // Anchor to the ABSOLUTE grid line so the selection stays glued to its text as the viewport
        // scrolls: a viewport row `r` shows absolute line `r - display_offset`.
        let line = self.grid.source_row(row)?;
        Some(selection::Cell { col, line })
    }

    pub fn selection_rects(&self, surf_w: f32, surf_h: f32) -> Vec<(f32, f32, f32, f32)> {
        self.live_selections()
            .flat_map(|(_, sel)| self.selection_range_rects(sel, surf_w, surf_h))
            .collect()
    }

    pub fn selection_active_rects(&self, surf_w: f32, surf_h: f32) -> Vec<(f32, f32, f32, f32)> {
        self.selection
            .filter(|s| s.dragged)
            .map_or_else(Vec::new, |s| self.selection_range_rects(&s, surf_w, surf_h))
    }

    fn selection_range_rects(
        &self,
        sel: &Selection,
        surf_w: f32,
        surf_h: f32,
    ) -> Vec<(f32, f32, f32, f32)> {
        let (cell_w, cell_h, cols, rows) = match self.cell_logical(surf_w, surf_h) {
            Some(t) => t,
            None => return Vec::new(),
        };
        if sel.kind == selection::SelectionKind::Column {
            let (start, end) = sel.ordered();
            return (0..rows)
                .filter_map(|row| {
                    let line = self.grid.source_row(row)?;
                    if !(start.line..=end.line).contains(&line) {
                        return None;
                    }
                    let mut left = start.col;
                    let mut right = end.col;
                    for cell in self
                        .grid
                        .reading_cells(line)
                        .into_iter()
                        .filter(|c| c.col <= end.col && c.col + c.width > start.col)
                    {
                        left = left.min(cell.col);
                        right = right.max(cell.col + cell.width - 1);
                    }
                    right = right.min(cols - 1);
                    Some((
                        (left + self.gutter_cells) as f32 * cell_w,
                        row as f32 * cell_h,
                        (right + 1 - left) as f32 * cell_w,
                        cell_h,
                    ))
                })
                .collect();
        }
        // Project the absolute-line selection back through the current scroll position so the
        // highlight rides the content (and clips at the viewport edges) as the user scrolls.
        let off = self
            .grid
            .visible_index(-(self.grid.display_offset() as i32));
        let mut projected = *sel;
        projected.anchor.line = self.grid.visible_index(sel.anchor.line);
        projected.head.line = self.grid.visible_index(sel.head.line);
        // The gutter strip sits left of the text — shift highlight rects past it.
        let gx = self.gutter_cells as f32 * cell_w;
        selection::selection_rects(&projected, cols, cell_w, cell_h, -off, rows)
            .into_iter()
            .map(|(x, y, w, h)| (x + gx, y, w, h))
            .collect()
    }
}
