//! Search retained rows using the same grapheme/cell mapper as visible text marks.
use super::TermGrid;
use crate::{
    matching::Matcher,
    search::{Match, SearchOptions},
};
impl TermGrid {
    pub(crate) fn search_matches(&self, query: &str, opts: SearchOptions) -> Vec<Match> {
        if query.is_empty() {
            return Vec::new();
        }
        let Ok(matcher) = Matcher::new(query, opts, None) else {
            return Vec::new();
        };
        self.search_with(&matcher)
    }
    pub(crate) fn search_with(&self, matcher: &Matcher) -> Vec<Match> {
        let mut result = Vec::new();
        let (first, last) = self.reading_rows();
        for line in first..=last {
            result.extend(matcher.row(&self.reading_cells(line), line));
        }
        result
    }
}
