//! WindTerm-style compressed scrollback store (P0b, strategy A).
//!
//! A long-history store that replaces alacritty's raw `Row<Cell>` ring (24 B/cell,
//! pre-allocated) with a **compressed + deduplicated** row store:
//!
//!   * rows are RLE-encoded into [`LineData`] (chars + style runs) — a blank 200-col line
//!     costs a few dozen bytes instead of 200×24 B;
//!   * identical rows are interned (shared `Arc`) — a repeated/blank line is ~free;
//!   * the row table is a power-of-two ring: contiguous `Vec`, bit-mask indexing
//!     (`idx & mask`, no modulo), capacity grows with history, oldest-row overwrite
//!     at the hard limit — no per-row heap churn;
//!   * a byte budget bounds the store; when exceeded, the oldest rows are dropped.
//!
//! This module is deliberately free of `alacritty_terminal`/`slint` types: the wire
//! conversion (`Row<Cell>` ↔ `LineData` ↔ `RenderCell`) lives in [`crate::grid`].

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
mod text;
pub use text::LineText;
pub(crate) mod background;

/// A style run covering columns `[start, start+len)`.
///
/// `wide` marks a *pair* run (len == 2): the first column is the left half of a wide
/// (CJK) glyph, the second is its spacer. Non-wide runs have arbitrary len ≥ 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleRun {
    pub start: u16,
    pub len: u16,
    /// Packed RGBA (0xAABBGGRR little-endian byte layout, stored as `u32`).
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub wide: bool,
}

impl StyleRun {
    #[inline]
    pub fn end(&self) -> u16 {
        self.start + self.len
    }
}

/// A stored row: one char per column, non-overlapping style runs covering `[0, cols)`,
/// and the wrap-continuation flag. Hash/Eq are content-derived, so identical rows intern
/// to one shared `Arc`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineData {
    /// One UTF-16 code unit per column (BMP chars fit directly; non-BMP are stored as U+FFFD).
    /// Stored as `Arc<[u16]>` so rows with identical text but different styles can share the
    /// character payload (P2.5 text dedup).
    ///
    /// **Shorter than [`cols`](Self::cols) on purpose**: trailing columns that render exactly
    /// like an absent cell (space, default/transparent background, no underline, not wide) are
    /// not stored — they are implied by the row width. Terminal output is left-aligned, so this
    /// is the difference between a 200-column row and the ~40–80 columns a log line actually
    /// occupies; every read path pads back to `cols`.
    pub chars: LineText,
    /// Sparse full graphemes for non-BMP bases and combining payload; an empty
    /// string marks a display-only wide-wrap gap. Plain BMP rows stay compact.
    pub extras: Vec<(u16, String)>,
    pub runs: Vec<StyleRun>,
    /// Width of the source row in columns (the grid's column count when it was stored). Read
    /// paths pad `chars`/`runs` up to this width with default cells.
    pub cols: u16,
    pub wrap: bool,
}

impl LineData {
    /// A fully default (blank) row of `cols` columns — the single most common line in
    /// terminal output (prompt spacing, pagers, `clear`ed screens).
    pub fn blank(cols: usize) -> Self {
        LineData {
            chars: vec![' ' as u16; cols].into(),
            extras: Vec::new(),
            runs: vec![StyleRun {
                start: 0,
                len: cols as u16,
                fg: 0,
                bg: 0,
                bold: false,
                italic: false,
                underline: false,
                wide: false,
            }],
            cols: cols as u16,
            wrap: false,
        }
    }

    /// A blank row: all spaces, one default run, no attributes. Blank rows share a single
    /// `Arc` and cost ~0 bytes in the store. A *wrapped* blank row is excluded: sharing it would
    /// drop the continuation flag the next row's text and reflow depend on.
    pub fn is_blank(&self) -> bool {
        self.extras.is_empty()
            && !self.wrap
            && self.runs.len() == 1
            && self.runs[0].start == 0
            && self.runs[0].len as usize == self.chars.len()
            // Sharing the default blank must not erase a visible background.
            && self.runs[0].bg.to_le_bytes()[3] == 0
            && !self.runs[0].bold
            && !self.runs[0].italic
            && !self.runs[0].underline
            && !self.runs[0].wide
            && self.chars.iter().all(|&c| c == ' ' as u16)
    }

    #[inline]
    pub fn cols(&self) -> usize {
        self.cols as usize
    }
}

/// The compressed scrollback store.
///
/// `row(0)` is the oldest retained row, `row(rows()-1)` the newest. The ring capacity
/// grows in powers of two (from [`RING_CAP`] up to [`MAX_CAP`]) while the byte budget has
/// headroom; the *effective* row count is governed by the byte budget (oldest rows are
/// dropped to stay within it), and only at [`MAX_CAP`] do rows drop purely on count.
pub struct ScrollbackStore {
    /// One `Arc` per slot — no per-slot bookkeeping: the byte budget is tracked for the store
    /// as a whole (`bytes`), and the interner is the single owner of a row's payload, so a
    /// slot only needs the pointer (8 B/slot instead of 16 B/slot, i.e. 8 MiB instead of
    /// 16 MiB at the 2^20 ceiling).
    ring: Vec<Option<Arc<LineData>>>,
    mask: usize,
    head: usize, // index of the newest row (0..cap)
    len: usize,  // number of live rows
    cap: usize,  // power of two
    interner: HashMap<Arc<LineData>, usize>,
    char_interner: HashMap<Arc<[u16]>, usize>,
    prefixes: HashMap<Arc<[u16]>, usize>,
    prefix_hint: Option<[u16; 64]>,
    blank: Option<Arc<LineData>>,
    bytes: usize,
    budget: usize,
}

/// Initial ring capacity: 1,024 slots (8 KiB on 64-bit targets). Empty and short-lived
/// sessions avoid the former 1 MiB allocation; long histories grow geometrically.
const RING_CAP: usize = 1 << 10;
/// Absolute ring ceiling: 2^20 slots (~8 MB pointer table worst-case). Past this the
/// ring overwrites the oldest row regardless of budget — the per-pane hard line ceiling.
const MAX_CAP: usize = 1 << 20;

impl ScrollbackStore {
    /// A store bounded to roughly `budget` bytes of row content (shared blank/interned
    /// rows cost ~0). Oldest rows are dropped to honour the budget.
    pub fn new(budget: usize) -> Self {
        ScrollbackStore {
            ring: vec![None; RING_CAP],
            mask: RING_CAP - 1,
            head: 0,
            len: 0,
            cap: RING_CAP,
            interner: HashMap::new(),
            char_interner: HashMap::new(),
            prefixes: HashMap::new(),
            prefix_hint: None,
            blank: None,
            bytes: 0,
            budget,
        }
    }

    /// Insert a row, returning the number of rows dropped to honour capacity/budget
    /// (0 normally, >0 when the store was full and/or over budget).
    ///
    /// **Byte accounting is per unique payload.** A row that interning maps onto an existing
    /// `Arc` adds 0: the bytes are already on the books for the row that owns them. Otherwise a
    /// screenful of repeated output (progress bars, `docker ps` refreshes, the same log line)
    /// would burn the budget at full price while sharing one allocation.
    pub fn push(&mut self, mut data: LineData) -> usize {
        let blank = data.is_blank();
        let repeated = (!blank && self.len != 0)
            .then(|| self.ring[self.head].as_ref())
            .flatten()
            .filter(|line| line.as_ref() == &data)
            .cloned();
        let (line, bytes) = if blank {
            (self.blank_rc(data.cols()), 0)
        } else if let Some(line) = repeated {
            // Common repeated output needs one row lookup and no text-table churn.
            *self.interner.get_mut(&line).expect("retained repeated row") += 1;
            (line, 0)
        } else {
            let text_bytes = self.intern_chars(&mut data);
            let row_bytes = estimate(&data) - data.chars.len() * 2;
            let candidate_text = data.chars.clone();
            let (line, fresh) = match self.interner.entry(Arc::new(data)) {
                Entry::Occupied(mut entry) => {
                    *entry.get_mut() += 1;
                    (entry.key().clone(), false)
                }
                Entry::Vacant(entry) => {
                    let line = entry.key().clone();
                    entry.insert(1);
                    (line, true)
                }
            };
            let bytes = if fresh {
                row_bytes + text_bytes
            } else {
                self.release_text(&candidate_text);
                0
            };
            (line, bytes)
        };
        self.bytes += bytes;

        let mut dropped = 0;
        // 1) Byte budget first: drop oldest until within budget.
        while self.len > 0 && self.bytes > self.budget {
            self.drop_oldest();
            dropped += 1;
        }
        // 2) Grow the ring when at capacity but the budget still has headroom — the
        //    "history as long as possible" knob: short-line sessions grow past 2^16 rows
        //    instead of dropping the oldest prematurely.
        if self.len == self.cap && self.cap < MAX_CAP && self.bytes <= self.budget {
            self.grow();
        }
        // 3) Ring ceiling: only binds at MAX_CAP (or when the budget is enormous).
        while self.len > 0 && self.len == self.cap {
            self.drop_oldest();
            dropped += 1;
        }
        self.head = (self.head + 1) & self.mask;
        self.len += 1;
        self.ring[self.head] = Some(line);
        while self.len > 0 && self.bytes > self.budget {
            self.drop_oldest();
            dropped += 1;
        }
        dropped
    }

    /// The row at logical index `i` (0 = oldest), or `None` out of range. Clones the `Arc`
    /// so the caller holds the row safely while the store mutates.
    pub fn row(&self, i: usize) -> Option<Arc<LineData>> {
        if i < self.len {
            // (head - len + 1 + i) mod cap, computed without usize underflow on wrap.
            let idx = (self.head + self.cap + 1 - self.len + i) & self.mask;
            self.ring[idx].as_ref().cloned()
        } else {
            None
        }
    }

    /// Number of live rows.
    #[inline]
    pub fn rows(&self) -> usize {
        self.len
    }

    /// Estimated bytes currently retained (shared rows cost 0).
    #[inline]
    pub fn estimated_bytes(&self) -> usize {
        self.bytes
    }

    /// The byte budget this store honours (oldest rows dropped to stay within it).
    #[inline]
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Drop everything.
    pub fn clear(&mut self) {
        *self = Self::new(self.budget);
    }

    /// The shared blank row for `cols` columns (cached per column count).
    fn blank_rc(&mut self, cols: usize) -> Arc<LineData> {
        let reuse = self
            .blank
            .as_ref()
            .map(|b| b.cols() == cols)
            .unwrap_or(false);
        if !reuse {
            self.blank = Some(Arc::new(LineData::blank(cols)));
        }
        self.blank.as_ref().expect("blank set").clone()
    }

    /// Count retained text pieces rather than storing a second full LineText key.
    /// Both shared prefixes and repeated suffixes are reclaimed independently.
    fn intern_chars(&mut self, data: &mut LineData) -> usize {
        if data.chars.prefix.is_none() && data.chars.len() < 80 {
            return intern_piece(&mut self.char_interner, &mut data.chars.tail);
        }
        let mut text = data.chars.clone();
        if text.len() >= 80 {
            let mut prefix = [0_u16; 64];
            for (slot, value) in prefix.iter_mut().zip(text.iter()) {
                *slot = *value;
            }
            let known = self
                .prefixes
                .get_key_value(prefix.as_slice())
                .map(|(key, _)| key);
            // Unique long lines retain a single flat allocation. Split only after
            // observing reuse, so random output does not pay for useless prefixes.
            if known.is_some() || self.prefix_hint.as_ref() == Some(&prefix) {
                text = text.split_prefix(known);
            }
            self.prefix_hint = Some(prefix);
        }
        let mut bytes = intern_piece(&mut self.char_interner, &mut text.tail);
        if let Some(prefix) = &mut text.prefix {
            bytes += intern_piece(&mut self.prefixes, prefix);
        }
        data.chars = text;
        bytes
    }

    fn release_text(&mut self, text: &LineText) -> usize {
        let mut bytes = release_piece(&mut self.char_interner, &text.tail);
        if let Some(prefix) = &text.prefix {
            bytes += release_piece(&mut self.prefixes, prefix);
        }
        bytes
    }

    /// Double the ring capacity, re-laying live rows contiguously at `ring[0..len]`
    /// (oldest first) so the bit-mask indexing stays correct at the new size.
    fn grow(&mut self) {
        let new_cap = self.cap * 2;
        let mut ring = vec![None; new_cap];
        for (i, slot) in ring.iter_mut().enumerate().take(self.len) {
            let idx = (self.head + self.cap + 1 - self.len + i) & self.mask;
            *slot = self.ring[idx].take();
        }
        self.ring = ring;
        self.cap = new_cap;
        self.mask = new_cap - 1;
        self.head = self.len - 1; // newest row sits at the end of the contiguous region
    }

    /// Drop the oldest row, returning it.
    fn drop_oldest(&mut self) -> Option<Arc<LineData>> {
        if self.len == 0 {
            return None;
        }
        let idx = (self.head + self.cap + 1 - self.len) & self.mask;
        let old = self.ring[idx].take()?;
        self.len -= 1;
        if let Some(count) = self.interner.get_mut(&old) {
            *count -= 1;
            if *count == 0 {
                self.interner.remove(&old);
                let bytes = estimate(&old) - old.chars.len() * 2 + self.release_text(&old.chars);
                self.bytes = self.bytes.saturating_sub(bytes);
            }
        }
        Some(old)
    }
}

fn intern_piece(table: &mut HashMap<Arc<[u16]>, usize>, piece: &mut Arc<[u16]>) -> usize {
    match table.entry(piece.clone()) {
        Entry::Occupied(mut entry) => {
            *piece = entry.key().clone();
            *entry.get_mut() += 1;
            0
        }
        Entry::Vacant(entry) => {
            entry.insert(1);
            piece.len() * 2
        }
    }
}

fn release_piece(table: &mut HashMap<Arc<[u16]>, usize>, piece: &Arc<[u16]>) -> usize {
    let count = table.get_mut(piece).expect("retained text piece");
    *count -= 1;
    if *count == 0 {
        table.remove(piece);
        piece.len() * 2
    } else {
        0
    }
}

/// Rough byte cost of a row: chars (2 B each, trailing blanks already dropped) + runs +
/// fixed header. Shared blanks/interns cost 0 in the ring (the shared payload is counted
/// once, amortised away).
fn estimate(d: &LineData) -> usize {
    d.chars.len() * 2
        + d.runs.len() * std::mem::size_of::<StyleRun>()
        + std::mem::size_of::<LineData>()
        + d.extras
            .iter()
            .map(|(_, text)| std::mem::size_of::<(u16, String)>() + text.len())
            .sum::<usize>()
}

#[cfg(test)]
mod tests;
