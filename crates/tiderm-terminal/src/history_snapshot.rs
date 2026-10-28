//! Explicit immutable export of retained rows. Text payloads remain shared.
use crate::scrollback::LineData;
use std::io::{self, Write};
use std::sync::Arc;

pub struct HistorySnapshot {
    pub(crate) rows: Vec<Arc<LineData>>,
}

impl HistorySnapshot {
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// Stream UTF-8 without ANSI controls. Soft wraps join, hard line endings remain.
    /// The callback supplies cancellation and progress without retaining a full text copy.
    pub fn write_utf8(
        &self,
        output: &mut impl Write,
        limit: u64,
        mut progress: impl FnMut(usize) -> bool,
    ) -> io::Result<u64> {
        let mut bytes = 0;
        for (index, row) in self.rows.iter().enumerate() {
            if !progress(index) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let mut text = String::new();
            let mut run = 0;
            for col in 0..row.cols() {
                while run < row.runs.len() && row.runs[run].end() as usize <= col {
                    run += 1;
                }
                if row
                    .runs
                    .get(run)
                    .is_some_and(|r| r.wide && col == r.start as usize + 1)
                {
                    continue;
                }
                if let Some((_, extra)) = row.extras.iter().find(|(c, _)| *c as usize == col) {
                    text.extend(extra.chars().filter(|c| !c.is_control()));
                } else {
                    let ch = row
                        .chars
                        .get(col)
                        .and_then(|u| char::from_u32(*u as u32))
                        .unwrap_or(' ');
                    if !ch.is_control() {
                        text.push(ch);
                    }
                }
            }
            if !row.wrap {
                text.truncate(text.trim_end_matches(' ').len());
                text.push('\n');
            }
            bytes += text.len() as u64;
            if bytes > limit {
                return Err(io::Error::other("history export exceeds disk limit"));
            }
            output.write_all(text.as_bytes())?;
        }
        if !progress(self.rows.len()) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        Ok(bytes)
    }
}
