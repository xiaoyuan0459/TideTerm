//! Compiled row matcher shared by search and independent text-mark rules.
use crate::grid::reading::ReadCell;
use crate::search::{self, Match, SearchOptions};

pub(crate) struct Matcher {
    needle: String,
    regex: Option<regex_lite::Regex>,
    options: SearchOptions,
}
impl Matcher {
    pub(crate) fn new(
        query: &str,
        options: SearchOptions,
        limit: Option<usize>,
    ) -> Result<Self, String> {
        let regex = if options.regex {
            let pattern = search::regex_pattern(query, options);
            let mut builder = regex_lite::RegexBuilder::new(&pattern);
            if let Some(limit) = limit {
                builder.size_limit(limit);
            }
            Some(builder.build().map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self {
            needle: if options.case_sensitive || options.regex {
                query.into()
            } else {
                query.to_lowercase()
            },
            regex,
            options,
        })
    }

    pub(crate) fn row(&self, cells: &[ReadCell], line: i32) -> Vec<Match> {
        if self.needle.is_empty() {
            return Vec::new();
        }
        let folded = !self.options.regex && !self.options.case_sensitive;
        let mut text = String::new();
        let mut columns = Vec::new();
        for cell in cells {
            for ch in cell.text.chars() {
                let value = if folded {
                    ch.to_lowercase().collect::<String>()
                } else {
                    ch.to_string()
                };
                columns.extend(std::iter::repeat_n(
                    (cell.col, cell.col + cell.width),
                    value.len(),
                ));
                text.push_str(&value);
            }
        }
        let ranges: Vec<_> = if let Some(re) = &self.regex {
            re.find_iter(&text).map(|m| (m.start(), m.end())).collect()
        } else {
            text.match_indices(&self.needle)
                .map(|(at, _)| (at, at + self.needle.len()))
                .filter(|(s, e)| {
                    !self.options.whole_word || search::at_word_boundary(&text, *s, *e)
                })
                .collect()
        };
        ranges
            .into_iter()
            .filter_map(|(start, end)| {
                if start == end {
                    return None;
                }
                let (Some(&(start, _)), Some(&(_, end))) =
                    (columns.get(start), columns.get(end - 1))
                else {
                    return None;
                };
                Some(Match { line, start, end })
            })
            .collect()
    }
}
