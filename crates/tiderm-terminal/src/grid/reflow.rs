//! Temporary raw-window coordinates for a resize, never a second history model.
use super::{Column, Flags, Line, TermGrid};
use crate::selection::Cell;
use std::hash::{DefaultHasher, Hasher};

struct RowPosition {
    group: usize,
    offset: usize,
    width: usize,
}

pub(crate) struct ReflowLayout {
    boundary: i32,
    history: i32,
    rows: Vec<RowPosition>,
    groups: Vec<u64>,
    cursor: Cell,
    cursor_group: usize,
}

impl TermGrid {
    pub(crate) fn reflow_layout(&self) -> ReflowLayout {
        let grid = self.term.grid();
        let boundary = self.reflow_boundary();
        let mut rows = Vec::new();
        let mut groups = Vec::new();
        let mut hash = DefaultHasher::new();
        let mut offset = 0;
        for line in boundary..self.size.rows as i32 {
            let row = &grid[Line(line)];
            let wrapped = row[Column(self.size.cols - 1)]
                .flags
                .contains(Flags::WRAPLINE);
            let gap = row[Column(self.size.cols - 1)]
                .flags
                .contains(Flags::LEADING_WIDE_CHAR_SPACER);
            let width = self.size.cols - usize::from(gap);
            rows.push(RowPosition {
                group: groups.len(),
                offset,
                width,
            });
            let used = if wrapped {
                width
            } else {
                (0..width)
                    .rfind(|&col| {
                        let cell = &row[Column(col)];
                        !matches!(cell.c, ' ' | '\0') || cell.zerowidth().is_some()
                    })
                    .map_or(0, |col| col + 1)
            };
            for col in 0..used {
                let cell = &row[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                hash.write_u32(cell.c as u32);
                if let Some(marks) = cell.zerowidth() {
                    for mark in marks {
                        hash.write_u32(*mark as u32);
                    }
                }
                hash.write_u32(u32::MAX);
            }
            offset += width;
            if !wrapped || line == self.size.rows as i32 - 1 {
                groups.push(hash.finish());
                hash = DefaultHasher::new();
                offset = 0;
            }
        }
        let cursor = Cell {
            line: grid.cursor.point.line.0,
            col: grid.cursor.point.column.0,
        };
        let cursor_group = rows[(cursor.line - boundary) as usize].group;
        ReflowLayout {
            boundary,
            history: self.total_history() as i32,
            rows,
            groups,
            cursor,
            cursor_group,
        }
    }
}

impl ReflowLayout {
    pub(crate) fn map(&self, cell: Cell, next: &Self, first: i32) -> Option<Cell> {
        if cell.line < self.boundary {
            let line = cell.line + self.history - next.history;
            return (line >= first).then_some(Cell {
                line,
                col: cell.col,
            });
        }
        // The host may know Enter was submitted before the first output row exists.
        if cell.line == self.boundary + self.rows.len() as i32 && cell.col == 0 {
            return (cell.line == self.cursor.line + 1).then_some(Cell {
                line: next.cursor.line + 1,
                col: 0,
            });
        }
        let old = self.rows.get((cell.line - self.boundary) as usize)?;
        let group = old.group as i64 + next.cursor_group as i64 - self.cursor_group as i64;
        let group = usize::try_from(group).ok()?;
        // Reflow may evict a prefix at the raw history cap. Never reuse an old
        // coordinate for a surviving suffix or another logical line.
        if self.groups[old.group] != *next.groups.get(group)? {
            return None;
        }
        let offset = old.offset + cell.col.min(old.width);
        let mut last = None;
        for (index, row) in next
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.group == group)
        {
            if offset >= row.offset && offset < row.offset + row.width {
                return Some(Cell {
                    line: next.boundary + index as i32,
                    col: offset - row.offset,
                });
            }
            last = Some((index, row));
        }
        let (index, row) = last?;
        (offset == row.offset + row.width).then_some(Cell {
            line: next.boundary + index as i32,
            col: row.width,
        })
    }
}
