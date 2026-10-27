//! TermGrid: history.
use super::appearance::CellStyle;
use super::*;

impl TermGrid {
    // ---- P0b: compressed long history (WindTerm strategy A) ----------------------------

    /// Drain alacritty's raw history window into the compressed store once it reaches the
    /// live-window cap, then reset the window (`ESC[3J` → `grid.clear_history()`). The
    /// virtual offset is kept continuous so a view scrolled into history sees no jump.
    pub(super) fn drain_pending(&mut self) {
        self.drain_pending_above(self.live_window);
    }

    /// [`drain_pending`](Self::drain_pending) with an explicit minimum-history threshold;
    /// `1` drains everything. Used by [`resize`](Self::resize) before shrinking columns.
    pub(super) fn drain_pending_above(&mut self, min_history: usize) {
        let h = self.term.grid().history_size();
        if h < min_history {
            return;
        }
        let cols = self.size.cols;
        let mut pending = Vec::with_capacity(h);
        {
            let grid = self.term.grid();
            // Newest line is Line(-1); push oldest-first so store order is chronological.
            for i in (1..=h).rev() {
                let row = &grid[Line(-(i as i32))];
                pending.push(self.line_data_from_grid_row(row, cols));
            }
        }
        self.pending.extend(pending);
        if self.pending.len() >= PENDING_FLUSH_THRESHOLD {
            self.flush_pending();
        }
        // Direct history clear keeps the output parser's unfinished OSC/CSI/UTF-8 intact.
        self.term.grid_mut().clear_history();
        // Moving raw rows into the store does not change their virtual positions.
        self.dirty = true;
    }

    /// Flush deferred rows into the compressed store. Called at the end of `feed` and when
    /// the pending batch reaches [`PENDING_FLUSH_THRESHOLD`].
    pub(super) fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        self.store.push_batch(pending);
        // A worker batch may retire oldest rows; never reuse position-keyed cells
        // across that boundary. Queries synchronize before reading the new bounds.
        self.row_cache.get_mut().clear();
    }

    /// Total lines above the live edge: compressed store + pending + alacritty's raw history.
    #[inline]
    pub fn total_history(&self) -> usize {
        if self.alt_screen() {
            0
        } else {
            self.store.rows() + self.pending.len() + self.term.grid().history_size()
        }
    }

    /// Bytes currently retained in the compressed store (shared blank/interned rows cost 0).
    /// The hard ceiling is the budget returned by [`scrollback_budget`](Self::scrollback_budget)
    /// — 32 MB by default since P1, or the `with_history` override. The memory-budget harness
    /// asserts the store stays under this even on a long, mixed-history session.
    #[inline]
    pub fn scrollback_bytes(&self) -> usize {
        self.store.estimated_bytes()
    }

    /// The byte budget this grid's store was built with (P1 default or the `with_history`
    /// override). Exposed so the harness can assert retention never exceeds it.
    #[inline]
    pub fn scrollback_budget(&self) -> usize {
        self.store.budget()
    }

    /// The virtual scroll offset (0 = pinned to the live edge) into `total_history()`.
    #[inline]
    pub fn virtual_offset(&self) -> usize {
        self.viewport.offset()
    }

    /// Set the virtual offset, clamped to `[0, total_history]`. When the viewport sits inside
    /// alacritty's raw window the internal `display_offset` is kept in sync; when it scrolls
    /// into the compressed store, alacritty is pinned at the top of its window and the store
    /// supplies the older rows (the [`snapshot`](Self::snapshot) mapping handles the split).
    pub fn set_view_offset(&mut self, v: usize) {
        self.flush_pending();
        let changed = self.viewport.move_to(v, self.total_history());
        let h = self.term.grid().history_size();
        let cur = self.term.grid().display_offset();
        let target = self.viewport.offset().min(h);
        let delta = target as i64 - cur as i64;
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta as i32));
        }
        // The viewport changed even when the raw window didn't (store rows above it).
        self.dirty |= changed || delta != 0;
    }

    /// Convert an alacritty grid row into the compressed [`LineData`] form (RLE style runs,
    /// chars 1-per-column, '\0' normalised to ' ', wide pairs merged).
    ///
    /// Trailing columns that render exactly like an absent cell are **not stored**: terminal
    /// output is left-aligned, so a 200-column grid spends most of its row on padding. A cell
    /// is droppable when it is a plain space, on the default (transparent) background, not
    /// underlined and not a wide glyph — everything else stays, because it still paints.
    /// The row's real width travels in [`LineData::cols`], so every read path pads back.
    pub(super) fn line_data_from_grid_row(&self, row: &Row<Cell>, cols: usize) -> LineData {
        let wrap = cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        // Find the stored width first: trailing padding is the common case (a 200-column grid
        // holding a 40-column command), and building the row at exactly that width keeps
        // `chars.into()` allocation-free. Truncating afterwards would reallocate every row —
        // that alone cost ~30% of feed throughput when this trim was first added.
        let keep = self.stored_width(row, cols);
        if keep == 0 {
            // Nothing paints. A row that *continues* onto the next line still has to keep that
            // bit, so it cannot join the shared (wrap-less) blank — it costs one 64-byte header
            // and nothing else.
            if wrap {
                return LineData {
                    chars: Vec::new().into(),
                    extras: Vec::new(),
                    runs: Vec::new(),
                    cols: cols as u16,
                    wrap: true,
                };
            }
            // The shared blank row covers it at ~0 bytes.
            return LineData::blank(cols);
        }
        let mut chars = Vec::with_capacity(keep);
        let mut extras = Vec::new();
        let mut runs: Vec<StyleRun> = Vec::new();
        let mut current: Option<CellStyle> = None;
        for c in 0..keep {
            let cell = &row[Column(c)];
            let flags = cell.flags;
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                // A wide glyph wrapping to the next row leaves a display-only gap.
                extras.push((c as u16, String::new()));
            } else if ch as u32 > 0xFFFF || cell.zerowidth().is_some() {
                let mut text = ch.to_string();
                if let Some(marks) = cell.zerowidth() {
                    text.extend(marks);
                }
                extras.push((c as u16, text));
            }
            chars.push(if (ch as u32) <= 0xFFFF {
                ch as u16
            } else {
                0xFFFD
            });
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                // Merge the spacer column into the preceding wide pair.
                if let Some(run) = runs.last_mut()
                    && run.wide
                {
                    run.len += 1;
                }
                continue;
            }
            let style = self.cell_style(cell);
            if current == Some(style) && !style.is_wide() {
                // current is set only when a run is appended below.
                runs.last_mut().expect("current style owns a run").len += 1;
            } else {
                current = Some(style);
                runs.push(style.start_run(c as u16));
            }
        }
        LineData {
            chars: chars.into(),
            extras,
            runs,
            cols: cols as u16,
            wrap,
        }
    }

    /// Width this row must be stored at: everything after it is padding that renders exactly
    /// like an absent cell (space, transparent background, no underline, not a wide glyph and
    /// carrying no combining marks). Scanned from the right so the cost is proportional to the
    /// padding — which is most of the row.
    fn stored_width(&self, row: &Row<Cell>, cols: usize) -> usize {
        let mut keep = cols;
        while keep > 0 {
            let cell = &row[Column(keep - 1)];
            let flags = cell.flags;
            // A spacer column belongs to the wide pair on its left; both must be stored.
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                break;
            }
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            if ch != ' ' || cell.zerowidth().is_some() {
                break;
            }
            let style = self.cell_style(cell);
            // Foreground never shows through a space; background, underline and the wide bit do.
            if style.is_wide() || style.underline() || style.background_visible() {
                break;
            }
            keep -= 1;
        }
        // A wide glyph owns two columns: keep its spacer so the pair stays one `len == 2` run
        // (hit-testing, column selection and copy all skip the right half through that run).
        if keep > 0 && keep < cols && row[Column(keep - 1)].flags.contains(Flags::WIDE_CHAR) {
            keep += 1;
        }
        keep
    }
}
