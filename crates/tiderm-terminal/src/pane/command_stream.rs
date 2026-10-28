//! Command boundaries follow executed parser operations, including synchronized replay.
use super::Commands;
use crate::grid::{OutputEvent, TermGrid};

impl Commands {
    pub(in crate::pane) fn observe(&mut self, grid: &TermGrid, event: OutputEvent) {
        match event {
            OutputEvent::Scroll(rows) if !grid.alt_screen() => {
                self.shift(rows, grid.reading_rows().0)
            }
            OutputEvent::Scroll(_) => {}
            OutputEvent::Alternate => {
                self.startup = None;
                self.prompt = None;
                self.input = None;
                self.repainted = false;
            }
            OutputEvent::Clear {
                primary: true,
                reset,
            } => {
                let startup = !reset && self.startup.is_some();
                // ConPTY repaints the viewport after a resize with ED2. At an
                // input prompt that erases the live OSC B anchor without
                // replaying it; falling back to the visible prompt is the only
                // way completion recovers. A clear while a command runs has no
                // live input anchor and keeps the strict marker policy.
                // A layout transition may issue several ED2 repaints before B
                // returns. The first clear already consumed the exact anchor.
                let repainted = !reset && (self.input.is_some() || self.repainted);
                self.invalidate();
                if repainted {
                    self.repainted = true;
                }
                if startup {
                    self.startup = Some(Self::cursor(grid));
                }
            }
            OutputEvent::Clear { primary: false, .. } => {}
            OutputEvent::ShellMarker(marker) if !grid.alt_screen() => self.marker(grid, marker),
            OutputEvent::ShellMarker(_) => {}
        }
    }

    fn marker(&mut self, grid: &TermGrid, marker: u8) {
        // A-only hooks describe command blocks but do not supply input anchors.
        self.marked |= marker == b'B';
        // B restores an exact input anchor; C starts a command whose output must
        // not be guessed as a prompt. An A-only replay does not restore either.
        if matches!(marker, b'B' | b'C' | b'D') {
            self.repainted = false;
        }
        let cursor = Self::cursor(grid);
        match marker {
            b'A' => {
                self.startup_finish(grid);
                self.finish(cursor);
                self.prompt = Some(cursor);
                self.input = None;
            }
            b'B' => self.input = Some(cursor),
            b'C' => {
                if let Some(input) = self.input.take() {
                    let (title, truncated) = Self::range(grid, input, cursor, 4096, 512);
                    if !truncated {
                        self.begin(&title, self.prompt.unwrap_or(input), cursor);
                    }
                } else if let Some(block) = self.blocks.back_mut().filter(|b| b.end.is_none()) {
                    block.output = cursor;
                }
            }
            b'D' => {
                self.input = None;
                self.finish(cursor);
            }
            _ => {}
        }
    }
}
