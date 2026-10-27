//! A single decoded row cache shared by all counted word/sentence steps.
use super::TerminalPane;
use crate::selection::Cell;

pub(super) struct ReadScan<'a> {
    pane: &'a TerminalPane,
    line: i32,
    cells: Vec<(usize, char)>,
}
impl<'a> ReadScan<'a> {
    pub fn sentence_start(&self, p: Cell) -> Option<Cell> {
        (p.col == 0 && !self.pane.grid.line_wraps(p.line - 1)).then(|| Cell {
            col: self
                .pane
                .grid
                .reading_cells(p.line)
                .iter()
                .find(|c| !c.text.trim().is_empty())
                .map_or(0, |c| c.col),
            ..p
        })
    }
    pub fn new(pane: &'a TerminalPane) -> Self {
        Self {
            pane,
            line: i32::MIN,
            cells: Vec::new(),
        }
    }
    fn load(&mut self, line: i32) {
        if self.line == line {
            return;
        }
        self.line = line;
        self.cells = self
            .pane
            .grid
            .reading_cells(line)
            .into_iter()
            .map(|c| (c.col, c.text.chars().next().unwrap_or(' ')))
            .collect();
    }
    pub fn ch(&mut self, p: Cell) -> char {
        self.load(p.line);
        self.cells
            .binary_search_by_key(&p.col, |(col, _)| *col)
            .ok()
            .and_then(|at| self.cells.get(at))
            .map_or(' ', |(_, c)| *c)
    }
    pub fn step(&mut self, p: Cell, forward: bool) -> Cell {
        self.load(p.line);
        let at = self.cells.partition_point(|(col, _)| *col < p.col);
        let next = if forward {
            self.cells.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|at| self.cells.get(at))
        };
        if let Some((col, _)) = next {
            return Cell { col: *col, ..p };
        }
        let (first, last) = self.pane.grid.reading_rows();
        let line = (p.line + if forward { 1 } else { -1 }).clamp(first, last);
        if line == p.line {
            return p;
        }
        self.load(line);
        Cell {
            line,
            col: if forward {
                self.cells.first()
            } else {
                self.cells.last()
            }
            .map_or(0, |(col, _)| *col),
        }
    }
}
