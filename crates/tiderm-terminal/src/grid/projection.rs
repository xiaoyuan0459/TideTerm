//! O(log folds) source/visible conversion. Storage is bounded by command count.
use super::TermGrid;

#[derive(Clone, PartialEq, Eq)]
struct Span {
    first: i32,
    last: i32,
    before: i32,
}

#[derive(Default)]
pub(super) struct Projection {
    spans: Vec<Span>,
}

impl Projection {
    fn index(&self, line: i32, first: i32) -> i32 {
        let at = self.spans.partition_point(|s| s.first < line);
        match at.checked_sub(1).map(|i| &self.spans[i]) {
            Some(s) if line <= s.last => s.first - first - s.before,
            Some(s) => line - first - s.before - (s.last - s.first),
            None => line - first,
        }
    }
    fn source(&self, index: i32, first: i32) -> i32 {
        let at = self
            .spans
            .partition_point(|s| s.first - first - s.before < index);
        let hidden = at.checked_sub(1).map_or(0, |i| {
            let s = &self.spans[i];
            s.before + s.last - s.first
        });
        first + index + hidden
    }
}

impl TermGrid {
    pub(crate) fn set_fold_ranges(&mut self, ranges: Vec<(i32, i32)>) {
        let mut before = 0;
        let spans = ranges
            .into_iter()
            .filter(|(first, last)| last > first)
            .take(628) // 500 command roots plus at most 128 manual subregions.
            .map(|(first, last)| {
                let span = Span {
                    first,
                    last,
                    before,
                };
                before += last - first;
                span
            })
            .collect::<Vec<_>>();
        if spans != self.projection.spans {
            self.projection.spans = spans;
            self.dirty = true;
        }
    }
    pub(crate) fn visible_index(&self, line: i32) -> i32 {
        let first = self.reading_rows().0;
        if self.alt_screen() {
            line - first
        } else {
            self.projection.index(line, first)
        }
    }
    pub(crate) fn source_index(&self, index: i32) -> i32 {
        let first = self.reading_rows().0;
        if self.alt_screen() {
            first + index
        } else {
            self.projection.source(index, first)
        }
    }
    pub(crate) fn visible_step(&self, line: i32, delta: i32) -> i32 {
        let (first, last) = self.reading_rows();
        self.source_index((self.visible_index(line) + delta).clamp(0, self.visible_index(last)))
            .clamp(first, last)
    }
    pub(crate) fn source_row(&self, row: usize) -> Option<i32> {
        let line =
            self.source_index(self.visible_index(-(self.viewport.offset() as i32)) + row as i32);
        let (first, last) = self.reading_rows();
        (first..=last).contains(&line).then_some(line)
    }
    /// Application mouse reports use live source coordinates, never synthetic folds.
    pub(crate) fn mouse_row(&self, row: usize) -> Option<usize> {
        if self.alt_screen() || self.projection.spans.is_empty() {
            return Some(row);
        }
        let line = self.source_row(row)?;
        (line >= 0 && self.fold_marker(line).is_none()).then_some(line as usize)
    }
    pub(crate) fn screen_row(&self, line: i32) -> Option<usize> {
        if self.fold_hidden(line) {
            return None;
        }
        let row = self.visible_index(line) - self.visible_index(-(self.viewport.offset() as i32));
        (row >= 0 && row < self.size.rows as i32).then_some(row as usize)
    }
    pub(crate) fn fold_hidden(&self, line: i32) -> bool {
        if self.alt_screen() {
            return false;
        }
        self.projection
            .spans
            .iter()
            .any(|s| line > s.first && line <= s.last)
    }
    pub(crate) fn fold_marker(&self, line: i32) -> Option<usize> {
        if self.alt_screen() {
            return None;
        }
        self.projection
            .spans
            .iter()
            .find(|s| s.first == line)
            .map(|s| (s.last - s.first + 1) as usize)
    }
}
