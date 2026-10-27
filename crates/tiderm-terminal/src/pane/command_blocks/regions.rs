//! Manual regions are owned by completed command outputs, never by screen pixels.
use super::{Block, TerminalPane};
use crate::selection::{Cell, SelectionKind};

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FoldError {
    NoSelection = 1,
    Shape,
    Output,
    Crossing,
    Limit,
}

pub struct FoldRegion {
    pub id: u64,
    pub parent: u64,
    pub depth: usize,
    pub title: String,
    pub first: u64,
    pub last: u64,
    pub folded: bool,
    pub manual: bool,
}

pub(super) struct Region {
    pub id: u64,
    pub first: Cell,
    pub last: Cell,
    pub folded: bool,
}
impl Region {
    pub(super) fn range(&self) -> (i32, i32) {
        (self.first.line, self.last.line)
    }
}
pub(super) fn contains(a: (i32, i32), b: (i32, i32)) -> bool {
    a.0 <= b.0 && b.1 <= a.1
}

impl Block {
    pub(super) fn remap_regions(&mut self, map: &impl Fn(Cell) -> Option<Cell>) {
        let root = self.fold_range();
        if root.is_none() {
            self.folded = false;
        }
        self.regions.retain_mut(|region| {
            let (Some(first), Some(last)) = (map(region.first), map(region.last)) else {
                return false;
            };
            if first.line >= last.line
                || !root.is_some_and(|r| contains(r, (first.line, last.line)))
            {
                return false;
            }
            region.first = first;
            region.last = last;
            true
        });
        // Widening may collapse distinct source rows onto the same physical boundaries.
        let mut seen = Vec::new();
        self.regions.retain(|r| {
            let range = r.range();
            if seen.contains(&range)
                || root == Some(range)
                || seen.iter().any(|p: &(i32, i32)| {
                    range.0 <= p.1 && p.0 <= range.1 && !contains(*p, range) && !contains(range, *p)
                })
            {
                false
            } else {
                seen.push(range);
                true
            }
        });
    }
}

impl TerminalPane {
    pub fn fold_region_count(&self) -> usize {
        if self.grid.alt_screen() {
            0
        } else {
            self.commands
                .blocks
                .iter()
                .filter(|b| b.fold_range().is_some())
                .map(|b| 1 + b.regions.len())
                .sum()
        }
    }
    fn selected_fold_range(&self) -> Result<(usize, Cell, Cell), FoldError> {
        let selection = self
            .selection
            .filter(|s| s.dragged)
            .ok_or(FoldError::NoSelection)?;
        if self.selection_count() != 1 || selection.kind != SelectionKind::Stream {
            return Err(FoldError::Shape);
        }
        if self.grid.alt_screen() {
            return Err(FoldError::Output);
        }
        let (a, b) = selection.ordered();
        let range = (a.line, b.line);
        if a.line >= b.line {
            return Err(FoldError::Output);
        }
        let (at, block) = self
            .commands
            .blocks
            .iter()
            .enumerate()
            .find(|(_, b)| b.fold_range().is_some_and(|r| contains(r, range)))
            .ok_or(FoldError::Output)?;
        if block.fold_range() == Some(range) {
            return Err(FoldError::Crossing);
        }
        let mut ranges = Vec::new();
        for old in &block.regions {
            let previous = old.range();
            if previous == range
                || (range.0 <= previous.1
                    && previous.0 <= range.1
                    && !contains(range, previous)
                    && !contains(previous, range))
            {
                return Err(FoldError::Crossing);
            }
            ranges.push(previous);
        }
        if self
            .commands
            .blocks
            .iter()
            .map(|b| b.regions.len())
            .sum::<usize>()
            >= 128
        {
            return Err(FoldError::Limit);
        }
        ranges.push(range);
        if ranges.iter().any(|r| {
            1 + ranges
                .iter()
                .filter(|p| *p != r && contains(**p, *r))
                .count()
                > 8
        }) {
            return Err(FoldError::Limit);
        }
        Ok((
            at,
            Cell {
                line: a.line,
                col: 0,
            },
            Cell {
                line: b.line,
                col: self.grid.size().cols - 1,
            },
        ))
    }

    pub fn fold_selection_status(&self) -> Result<(), FoldError> {
        self.selected_fold_range().map(|_| ())
    }

    pub fn fold_selection(&mut self) -> Result<u64, FoldError> {
        let (at, first, last) = self.selected_fold_range()?;
        self.commands.next_id = self
            .commands
            .next_id
            .checked_add(1)
            .ok_or(FoldError::Limit)?;
        let id = self.commands.next_id;
        self.commands.blocks[at].regions.push(Region {
            id,
            first,
            last,
            folded: true,
        });
        self.folds_changed();
        Ok(id)
    }

    pub fn fold_regions(&self) -> Vec<FoldRegion> {
        if self.grid.alt_screen() {
            return Vec::new();
        }
        let line = |n| (self.grid.total_history() as i64 + i64::from(n) + 1).max(1) as u64;
        let mut rows = Vec::new();
        for block in &self.commands.blocks {
            let Some((first, last)) = block.fold_range() else {
                continue;
            };
            rows.push(FoldRegion {
                id: block.id,
                parent: 0,
                depth: 0,
                title: block.title.clone(),
                first: line(first),
                last: line(last),
                folded: block.folded,
                manual: false,
            });
            for region in &block.regions {
                let ancestors: Vec<_> = block
                    .regions
                    .iter()
                    .filter(|r| r.id != region.id && contains(r.range(), region.range()))
                    .collect();
                let parent = ancestors
                    .iter()
                    .min_by_key(|r| r.last.line - r.first.line)
                    .map_or(block.id, |r| r.id);
                let title = self
                    .grid
                    .reading_cells(region.first.line)
                    .into_iter()
                    .map(|c| c.text)
                    .collect::<String>()
                    .chars()
                    .take(120)
                    .collect::<String>();
                rows.push(FoldRegion {
                    id: region.id,
                    parent,
                    depth: ancestors.len() + 1,
                    title: title.trim().to_owned(),
                    first: line(region.first.line),
                    last: line(region.last.line),
                    folded: region.folded,
                    manual: true,
                });
            }
        }
        rows.sort_by_key(|r| (r.first, std::cmp::Reverse(r.last), r.depth));
        rows
    }

    pub fn fold_region_set(&mut self, id: u64, folded: bool, recursive: bool) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let mut found = false;
        for block in &mut self.commands.blocks {
            let target = if block.id == id {
                block.fold_range()
            } else {
                block.regions.iter().find(|r| r.id == id).map(Region::range)
            };
            let Some(target) = target else {
                continue;
            };
            if block.id == id {
                block.folded = folded;
            }
            for region in &mut block.regions {
                if region.id == id || (recursive && contains(target, region.range())) {
                    region.folded = folded;
                }
            }
            found = true;
            break;
        }
        if found {
            self.folds_changed();
        }
        found
    }

    pub fn fold_region_remove(&mut self, id: u64) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let mut found = false;
        for block in &mut self.commands.blocks {
            let before = block.regions.len();
            block.regions.retain(|r| r.id != id);
            found |= before != block.regions.len();
        }
        if found {
            self.folds_changed();
        }
        found
    }

    pub fn fold_region_jump(&mut self, id: u64) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let mut target = None;
        for block in &mut self.commands.blocks {
            if block.id == id {
                target = block.fold_range();
                break;
            }
            if let Some(region) = block.regions.iter().find(|r| r.id == id) {
                let range = region.range();
                target = Some(range);
                block.folded = false;
                for parent in &mut block.regions {
                    if parent.id != id && contains(parent.range(), range) {
                        parent.folded = false;
                    }
                }
                break;
            }
        }
        let Some((first, _)) = target else {
            return false;
        };
        self.folds_changed();
        self.reading.cursor = Cell {
            line: first,
            col: 0,
        };
        self.grid.scroll_to_visible(first);
        self.reading_clamp(false);
        true
    }
}
