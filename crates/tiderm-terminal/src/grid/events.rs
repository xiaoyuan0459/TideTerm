//! TermGrid: events.
use super::*;

impl TermGrid {
    /// Drain any terminal-originated replies (DSR/DA/etc.) the parser queued while
    /// processing the last [`feed`](Self::feed). The controller must write these back to
    /// the session's pty (`SessionManager::write`) or the shell can hang at startup.
    /// Returns an empty vec when there is nothing to forward.
    pub fn take_replies(&mut self) -> Vec<u8> {
        self.events.take_replies()
    }

    /// Whether a BEL arrived since the last call (G8: the pane raises a toast).
    pub fn take_bell(&mut self) -> bool {
        self.events.take_bell()
    }

    /// Drain queued OSC 52 clipboard requests for the pane to act on (G6). See
    /// [`OscEvent`] — store goes to the clipboard, load reads it and replies to the pty
    /// via [`TermGrid::reply_to_pty`].
    pub fn drain_osc_events(&mut self) -> Vec<OscEvent> {
        self.events.take_clipboard()
    }

    /// Queue a grid-originated write destined for the pty (used by OSC 52 load replies;
    /// drained by the controller through [`TermGrid::take_replies`]).
    pub fn reply_to_pty(&self, bytes: Vec<u8>) {
        self.events.reply(&bytes);
    }

    /// Drain pending OSC 0;2 window-title updates, keeping only the latest (intermediate
    /// titles from rapid app-side rewrites collapse). `None` when nothing arrived since
    /// the last call — the controller drains this every frame to refresh the pane
    /// header / tab label. An empty string explicitly restores the host's default
    /// title; it is distinct from `None` (no update).
    pub fn take_title(&mut self) -> Option<String> {
        self.events.take_title()
    }

    /// Reflow the grid to `cols`×`rows`. Returns `true` if the size actually changed (the
    /// caller should then also resize the *session* via `SessionManager::resize`).
    pub fn resize(&mut self, cols: usize, rows: usize) -> bool {
        // 2 列下限：alacritty 缩列 reflow 在 columns == 1 时对含宽字符（中文）的行
        // 死循环 —— `Row::shrink` 每轮剥 1 列，但宽字符 spacer 分支往 wrapped 头部
        // insert 回一格，净减 0 ⇒ 行数无限增长（2026-09-28 探针实测 8 GiB 分配
        // abort）。columns ≥ 2 时每轮净减 ≥ 1，循环天然有界；历史已全量排进压缩层。
        let cols = cols.max(2);
        let rows = rows.max(1);
        if cols == self.size.cols && rows == self.size.rows {
            return false;
        }
        if cols < self.size.cols {
            // Bound shrinking reflow to the live rows. Widening must still join
            // the raw wrapped history back into the restored viewport.
            self.prepare_column_resize();
        }
        self.row_cache.get_mut().clear();
        let anchor = self
            .viewport
            .is_held()
            .then(|| self.total_history().saturating_sub(self.viewport.offset()));
        self.size = TermSize { cols, rows };
        self.term.resize(self.size);
        if let Some(anchor) = anchor {
            self.set_view_offset(self.total_history().saturating_sub(anchor));
        }
        self.dirty = true;
        true
    }

    /// Take-and-clear the dirty flag. Also clears alacritty's accumulated damage so it
    /// doesn't grow unbounded (the renderers repaint the whole pane texture per dirty
    /// frame — partial-damage repaint is a noted future refinement).
    pub fn take_dirty(&mut self) -> bool {
        let d = self.dirty;
        self.dirty = false;
        self.term.reset_damage();
        d
    }

    pub fn size(&self) -> TermSize {
        self.size
    }
}
