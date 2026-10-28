//! Command output roots and laminar manual subregions share one visible projection.
use super::{regions::contains, Block, TerminalPane};

impl Block {
    pub(super) fn fold_range(&self) -> Option<(i32, i32)> {
        let end = self.end?;
        let first = self.output.line + i32::from(self.output.col != 0);
        let last = end.line - 1;
        (last > first).then_some((first, last))
    }
}

impl TerminalPane {
    /// At most one root per visible row, using the same source projection as selection.
    pub fn command_fold_markers(&self) -> Vec<(u64, usize, bool)> {
        if self.grid.alt_screen() {
            return Vec::new();
        }
        self.commands
            .blocks
            .iter()
            .filter_map(|b| {
                let (first, _) = b.fold_range()?;
                Some((b.id, self.grid.screen_row(first)?, b.folded))
            })
            .collect()
    }

    pub(in crate::pane) fn sync_command_folds(&mut self) {
        let mut ranges = Vec::new();
        for block in &self.commands.blocks {
            if block.folded
                && let Some(range) = block.fold_range()
            {
                ranges.push(range);
            }
            ranges.extend(block.regions.iter().filter(|r| r.folded).map(|r| r.range()));
        }
        ranges.sort_by_key(|r| (r.0, std::cmp::Reverse(r.1)));
        let mut visible: Vec<(i32, i32)> = Vec::new();
        for range in ranges {
            if visible
                .last()
                .is_none_or(|parent| !contains(*parent, range))
            {
                visible.push(range);
            }
        }
        self.grid.set_fold_ranges(visible);
    }

    pub(super) fn folds_changed(&mut self) {
        self.selection_clear();
        self.reading.visual = None;
        self.reading.operator = None;
        self.sync_command_folds();
        if self.reading.active {
            self.reading.cursor.line = self.grid.visible_step(self.reading.cursor.line, 0);
            self.reading_clamp(false);
        }
    }

    pub fn command_set_folded(&mut self, id: u64, folded: bool) -> bool {
        if !self
            .commands
            .blocks
            .iter()
            .any(|b| b.id == id && b.fold_range().is_some())
        {
            return false;
        }
        self.fold_region_set(id, folded, false)
    }

    pub fn command_fold_current(&mut self, folded: Option<bool>) -> bool {
        self.command_fold_current_recursive(folded, false)
    }

    /// Uppercase reading commands explicitly operate on the whole command subtree.
    pub fn command_fold_current_recursive(
        &mut self,
        folded: Option<bool>,
        recursive: bool,
    ) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let line = if self.reading.active {
            self.reading.cursor.line
        } else {
            self.command_cursor().line
        };
        let Some(block) = self
            .commands
            .blocks
            .iter()
            .rev()
            .find(|b| b.prompt.line <= line)
            .filter(|b| b.fold_range().is_some())
        else {
            return false;
        };
        if recursive {
            return self.fold_region_set(block.id, folded.unwrap_or(!block.folded), true);
        }
        let Some(range) = block.fold_range() else {
            return false;
        };
        let root = (block.id, range, block.folded);
        let mut candidates = vec![root];
        candidates.extend(
            block
                .regions
                .iter()
                .filter(|r| (r.first.line..=r.last.line).contains(&line))
                .map(|r| (r.id, r.range(), r.folded)),
        );
        let target = if folded == Some(true) {
            candidates
                .into_iter()
                .filter(|r| !r.2)
                .min_by_key(|r| r.1 .1 - r.1 .0)
                .map(|r| (r.0, true))
        } else if let Some(closed) = candidates
            .iter()
            .filter(|r| r.2)
            .max_by_key(|r| r.1 .1 - r.1 .0)
        {
            Some((closed.0, false))
        } else if folded.is_none() {
            candidates
                .into_iter()
                .min_by_key(|r| r.1 .1 - r.1 .0)
                .map(|r| (r.0, true))
        } else {
            None
        };
        target.is_some_and(|(id, state)| self.fold_region_set(id, state, false))
    }

    pub fn command_fold_all(&mut self, folded: bool) {
        if self.grid.alt_screen() {
            return;
        }
        for block in &mut self.commands.blocks {
            if block.fold_range().is_some() {
                block.folded = folded;
            }
            for region in &mut block.regions {
                region.folded = folded;
            }
        }
        self.folds_changed();
    }

    /// Reveal the entire ancestor path containing a searched/read source cell.
    pub(in crate::pane) fn command_unfold_line(&mut self, line: i32) {
        let mut changed = false;
        for block in &mut self.commands.blocks {
            if block.folded
                && block
                    .fold_range()
                    .is_some_and(|(first, last)| (first..=last).contains(&line))
            {
                block.folded = false;
                changed = true;
            }
            for region in &mut block.regions {
                if region.folded && (region.first.line..=region.last.line).contains(&line) {
                    region.folded = false;
                    changed = true;
                }
            }
        }
        if changed {
            self.folds_changed();
        }
    }
}
