//! The existing search controller, kept separate from independent mark rules.
use super::{search, selection, TerminalPane};
impl TerminalPane {
    // ---- In-pane search (Ctrl+F) ------------------------------------------------------------
    //
    // Open a search box, type to find/highlight matches across the grid + scrollback, and step
    // through them (Enter / Shift+Enter), revealing each by scrolling it into view. Mirrors the
    // xterm `@xterm/addon-search` wiring in the Electron `Terminal.tsx` / `SearchBox.tsx`.

    /// Open the search box (Ctrl+F). The query starts empty; type to search.
    pub fn search_open(&mut self) {
        self.search_shown = true;
    }

    /// Close the search box, dropping the query/matches and pinning the viewport back to the
    /// bottom (the live prompt).
    pub fn search_close(&mut self) {
        self.search_shown = false;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_index = None;
        self.grid.scroll_to_bottom();
    }

    /// Whether the search box is open.
    pub fn search_is_open(&self) -> bool {
        self.search_shown
    }

    /// The current query text.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Set the query (find-as-you-type): recompute matches across the grid + scrollback, pick the
    /// match nearest the current viewport, and scroll it into view.
    pub fn search_set_query(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.search_recompute();
        self.search_reveal_active();
    }

    /// The active search switches (for the box's toggle state).
    pub fn search_options(&self) -> search::SearchOptions {
        self.search_options
    }

    /// Flip the Aa / .* / word toggles: recompute matches with the new behaviour and
    /// re-reveal the nearest match.
    pub fn search_set_options(&mut self, opts: search::SearchOptions) {
        if self.search_options == opts {
            return;
        }
        self.search_options = opts;
        self.search_recompute();
        self.search_reveal_active();
    }

    /// Step to the next (`forward`) / previous match, wrapping around, and reveal it.
    pub fn search_step(&mut self, forward: bool) {
        if self.search_matches.is_empty() {
            return;
        }
        let i = self.search_index.unwrap_or(0);
        self.search_index = Some(search::step(i, self.search_matches.len(), forward));
        self.search_reveal_active();
    }

    /// `(current_1_based, total)` for the match counter — `(0, 0)` when there are no matches.
    pub fn search_count(&self) -> (usize, usize) {
        let total = self.search_matches.len();
        let cur = self.search_index.map(|i| i + 1).unwrap_or(0);
        (cur, total)
    }

    // pre-existing; deferred per repo lint policy (test.yml)
    #[allow(clippy::type_complexity)]
    pub fn search_view_rects(
        &self,
        surf_w: f32,
        surf_h: f32,
    ) -> (Vec<(f32, f32, f32, f32)>, Option<(f32, f32, f32, f32)>) {
        let (cell_w, cell_h, _cols, _rows) = match self.cell_logical(surf_w, surf_h) {
            Some(t) => t,
            None => return (Vec::new(), None),
        };
        // 搜索高亮矩形右移 gutter 条宽（与选区高亮同源）。
        let gx = self.gutter_cells as f32 * cell_w;
        let mut rects = Vec::new();
        let mut active = None;
        for (i, m) in self.search_matches.iter().enumerate() {
            let Some(row) = self.grid.screen_row(m.line) else {
                continue;
            };
            let rect = (
                gx + m.start as f32 * cell_w,
                row as f32 * cell_h,
                (m.end.saturating_sub(m.start)) as f32 * cell_w,
                cell_h,
            );
            if Some(i) == self.search_index {
                active = Some(rect);
            } else {
                rects.push(rect);
            }
        }
        (rects, active)
    }

    /// Recompute `search_matches` for the current query, choosing an initial active match nearest
    /// the viewport top. Clears everything for an empty query.
    pub(super) fn search_recompute(&mut self) {
        if self.search_query.is_empty() {
            self.search_matches.clear();
            self.search_index = None;
            return;
        }
        self.search_matches = self
            .grid
            .search_matches(&self.search_query, self.search_options);
        self.search_index = if self.search_matches.is_empty() {
            None
        } else {
            let prefer_line = -(self.grid.display_offset() as i32);
            search::initial_index(&self.search_matches, prefer_line)
        };
    }

    /// Scroll the active match into view (no-op if already visible or there's none).
    pub(super) fn search_reveal_active(&mut self) {
        if let Some(m) = self
            .search_index
            .and_then(|i| self.search_matches.get(i).copied())
        {
            self.command_unfold_line(m.line);
            self.grid.scroll_to_visible(m.line);
            if self.reading.active {
                self.reading_move(selection::Cell {
                    line: m.line,
                    col: m.start,
                });
            }
        }
    }

    /// Recompute matches against the (possibly reflowed) grid — call after a [`resize`](Self::resize)
    /// so the highlight rects keep tracking the rewrapped text while the search box stays open. A
    /// no-op when search is closed; doesn't force-scroll (the viewport stays where the user left it).
    pub fn search_reflow(&mut self) {
        if self.search_shown {
            self.search_recompute();
        }
    }
}
