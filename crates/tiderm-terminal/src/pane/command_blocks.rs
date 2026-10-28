//! Bounded main-buffer command positions. Coordinates move with the existing
//! viewport scroll accounting; compressed-history transfers do not move them.
mod actions;
mod folding;
mod input;
mod regions;
mod startup;
#[path = "command_stream.rs"]
mod stream;
use super::TerminalPane;
use crate::selection::Cell;
pub use regions::{FoldError, FoldRegion};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct CommandBlock {
    pub id: u64,
    pub title: String,
    pub line: u64,
    pub finished: bool,
    pub folded: bool,
    pub foldable: bool,
    pub startup: bool,
}

struct Block {
    id: u64,
    title: String,
    prompt: Cell,
    output: Cell,
    end: Option<Cell>,
    folded: bool,
    regions: Vec<regions::Region>,
    startup: bool,
}

#[derive(Default)]
pub(super) struct Commands {
    blocks: VecDeque<Block>,
    next_id: u64,
    prompt: Option<Cell>,
    input: Option<Cell>,
    pub(super) startup: Option<Cell>,
    marked: bool,
    /// A repaint erased the live input anchor without replaying OSC B. The
    /// conventional visible prompt is authoritative until shell markers resume.
    repainted: bool,
}

impl Commands {
    pub(super) fn has_folded(&self) -> bool {
        self.blocks
            .iter()
            .any(|b| b.folded || b.regions.iter().any(|r| r.folded))
    }
    pub fn remap(&mut self, map: impl Fn(Cell) -> Option<Cell>) {
        self.blocks.retain_mut(|b| {
            let (Some(prompt), Some(output)) = (map(b.prompt), map(b.output)) else {
                return false;
            };
            let end = match b.end {
                Some(end) => match map(end) {
                    Some(end) => Some(end),
                    None => return false,
                },
                None => None,
            };
            b.prompt = prompt;
            b.output = output;
            b.end = end;
            b.remap_regions(&map);
            true
        });
        let remapped_prompt = self.prompt.and_then(&map);
        let remapped_input = self.input.and_then(&map);
        // 重排把锚点映射丢了（提示符首行被折叠/滚出可映射区）而 shell 又不会补发
        // OSC B 时，会留下“markers 权威、租约已失”的死状态：`visible_input` 认定
        // 标记可信却没有锚点，于是补全一直不恢复。此时与 ConPTY 重绘同理，回退到
        // 可见提示符；下一次 B 到来会重新收紧到精确锚点。
        if self.input.is_some() && (remapped_input.is_none() || remapped_prompt.is_none()) {
            self.repainted = true;
        }
        self.prompt = remapped_prompt;
        self.input = if self.prompt.is_none() {
            None
        } else {
            remapped_input
        };
        self.startup = self.startup.and_then(map);
    }

    pub fn invalidate(&mut self) {
        self.blocks.clear();
        self.prompt = None;
        self.input = None;
        self.startup = None;
        self.repainted = false;
    }

    pub fn shift(&mut self, scrolled: usize, first: i32) {
        self.shift_rows(i32::try_from(scrolled).unwrap_or(i32::MAX), first);
    }

    pub fn shift_rows(&mut self, shift: i32, first: i32) {
        let move_cell = |cell: &mut Cell| cell.line = cell.line.saturating_sub(shift);
        for block in &mut self.blocks {
            move_cell(&mut block.prompt);
            move_cell(&mut block.output);
            if let Some(end) = &mut block.end {
                move_cell(end);
            }
            block.remap_regions(&|cell| {
                let line = cell.line.saturating_sub(shift);
                (line >= first).then_some(Cell { line, ..cell })
            });
        }
        for cell in [&mut self.prompt, &mut self.input].into_iter().flatten() {
            move_cell(cell);
        }
        if let Some(cell) = &mut self.startup {
            move_cell(cell);
            if cell.line < first {
                self.startup = None;
            }
        }
        self.blocks.retain(|b| b.prompt.line >= first);
        if self.prompt.is_some_and(|c| c.line < first) {
            self.prompt = None;
            self.input = None;
        }
    }

    fn finish(&mut self, end: Cell) {
        if let Some(block) = self.blocks.back_mut().filter(|b| b.end.is_none()) {
            block.end = Some(end);
        }
    }

    fn begin(&mut self, title: &str, prompt: Cell, output: Cell) {
        let title = title.trim();
        if title.is_empty() || title.len() > 4096 || title.contains('\0') {
            return;
        }
        // Enter may have created the block before the shell emits OSC 133 C.
        if let Some(last) = self
            .blocks
            .back_mut()
            .filter(|b| b.end.is_none() && b.prompt == prompt)
        {
            last.title = title.to_owned();
            last.output = output;
            return;
        }
        self.finish(prompt);
        let Some(id) = self.next_id.checked_add(1) else {
            return;
        };
        self.next_id = id;
        self.blocks.push_back(Block {
            id: self.next_id,
            title: title.to_owned(),
            prompt,
            output,
            end: None,
            folded: false,
            regions: Vec::new(),
            startup: false,
        });
        while self.blocks.len() > 500 {
            self.blocks.pop_front();
        }
    }
}

impl TerminalPane {
    fn command_cursor(&self) -> Cell {
        Commands::cursor(&self.grid)
    }

    /// Called only after the host recognizes a submitted shell prompt, never for
    /// arbitrary keyboard/IME/password input. OSC 133 supplies richer boundaries.
    pub fn command_submitted(&mut self, title: &str) {
        if self.grid.alt_screen() {
            return;
        }
        self.commands.input = None;
        // A submitted command consumes the repaint fallback; the next prompt
        // must supply a fresh OSC B anchor before exact input resumes.
        self.commands.repainted = false;
        self.command_startup_finish();
        let cursor = self.command_cursor();
        let mut prompt = self.commands.prompt.unwrap_or(Cell {
            line: cursor.line,
            col: 0,
        });
        if self.commands.prompt.is_none() {
            for _ in 0..512 {
                if !self.grid.line_wraps(prompt.line - 1) {
                    break;
                }
                prompt.line -= 1;
            }
        }
        self.commands.begin(
            title,
            prompt,
            Cell {
                line: cursor.line + 1,
                col: 0,
            },
        );
    }

    pub fn command_blocks(&self) -> Vec<CommandBlock> {
        if self.grid.alt_screen() {
            return Vec::new();
        }
        self.commands
            .blocks
            .iter()
            .rev()
            .map(|b| CommandBlock {
                id: b.id,
                title: b.title.clone(),
                line: (self.grid.total_history() as i64 + i64::from(b.prompt.line) + 1).max(1)
                    as u64,
                finished: b.end.is_some(),
                folded: b.folded,
                foldable: b.folded || b.fold_range().is_some(),
                startup: b.startup,
            })
            .collect()
    }

    pub fn command_jump(&mut self, id: u64) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let Some(target) = self
            .commands
            .blocks
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.prompt)
        else {
            return false;
        };
        self.reading_move(target);
        true
    }

    pub fn command_step(&mut self, forward: bool) -> bool {
        if self.grid.alt_screen() {
            return false;
        }
        let line = if self.reading.active {
            self.reading.cursor.line
        } else {
            -(self.grid.display_offset() as i32)
        };
        let target = if forward {
            self.commands.blocks.iter().find(|b| b.prompt.line > line)
        } else {
            self.commands
                .blocks
                .iter()
                .rev()
                .find(|b| b.prompt.line < line)
        }
        .map(|b| b.id);
        target.is_some_and(|id| self.command_jump(id))
    }

    /// Copy is capped at 64 KiB/1024 rows; preview at 8 KiB/32 rows. The caller
    /// must surface truncation. No duplicate terminal output is retained here.
    pub fn command_output(&self, id: u64, preview: bool) -> Option<(String, bool)> {
        if self.grid.alt_screen() {
            return None;
        }
        let block = self.commands.blocks.iter().find(|b| b.id == id)?;
        let end = block.end.unwrap_or_else(|| self.command_cursor());
        Some(self.command_range(
            block.output,
            end,
            if preview { 8192 } else { 65536 },
            if preview { 32 } else { 1024 },
        ))
    }

    fn command_range(&self, start: Cell, end: Cell, budget: usize, rows: usize) -> (String, bool) {
        Commands::range(&self.grid, start, end, budget, rows)
    }
}

impl Commands {
    fn cursor(grid: &crate::grid::TermGrid) -> Cell {
        Cell {
            line: (grid.output_line_number() as i64 - grid.total_history() as i64 - 1) as i32,
            col: grid.cursor_boundary_col(),
        }
    }

    fn range(
        grid: &crate::grid::TermGrid,
        start: Cell,
        end: Cell,
        budget: usize,
        rows: usize,
    ) -> (String, bool) {
        let mut text = String::new();
        let last = if end.col == 0 { end.line - 1 } else { end.line };
        if start.line < grid.reading_rows().0 || last < start.line {
            return (text, false);
        }
        let mut truncated = false;
        for (count, line) in (start.line..=last).enumerate() {
            if count >= rows {
                truncated = true;
                break;
            }
            let col = if line == start.line { start.col } else { 0 };
            let stop = if line == end.line {
                end.col.saturating_sub(1)
            } else {
                usize::MAX
            };
            let Some(value) = grid.selected_cells(line, col, stop) else {
                truncated = true;
                break;
            };
            let wrapped = grid.line_wraps(line);
            let value = if wrapped {
                value.as_str()
            } else {
                value.trim_end()
            };
            for ch in value.chars() {
                if text.len() + ch.len_utf8() > budget {
                    truncated = true;
                    break;
                }
                text.push(ch);
            }
            if truncated {
                break;
            }
            if !wrapped && line < last {
                if text.len() == budget {
                    truncated = true;
                    break;
                }
                text.push('\n');
            }
        }
        (text, truncated)
    }
}

#[cfg(test)]
mod remap_tests {
    use super::*;

    #[test]
    fn reflow_that_drops_the_anchor_reopens_the_visible_prompt_fallback() {
        let mut commands = Commands {
            marked: true,
            prompt: Some(Cell { col: 0, line: 4 }),
            input: Some(Cell { col: 6, line: 4 }),
            ..Default::default()
        };
        assert!(!commands.repainted);
        // 重排无法映射锚点（提示符首行被折叠或滚出可映射区）：不能留下
        // “markers 权威、租约已失”的死状态，否则补全要等 shell 补发 OSC B 才恢复。
        commands.remap(|_| None);
        assert!(commands.input.is_none());
        assert!(
            commands.repainted,
            "dropped anchor must fall back to the visible prompt"
        );
    }

    #[test]
    fn reflow_that_keeps_the_anchor_stays_exact() {
        let mut commands = Commands {
            marked: true,
            prompt: Some(Cell { col: 0, line: 4 }),
            input: Some(Cell { col: 6, line: 4 }),
            ..Default::default()
        };
        commands.remap(|cell| {
            Some(Cell {
                line: cell.line - 1,
                ..cell
            })
        });
        assert_eq!(commands.input, Some(Cell { col: 6, line: 3 }));
        assert!(!commands.repainted, "a mapped anchor stays exact");
    }
}
