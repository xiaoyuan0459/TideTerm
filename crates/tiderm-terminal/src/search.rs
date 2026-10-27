//! In-pane substring search over the terminal grid + scrollback — the model behind Ctrl+F.
//!
//! A pure match finder (case-insensitive substring) plus the bookkeeping the pane needs to
//! step through results. The grid/scrollback text is supplied by
//! [`TerminalPane`](crate::pane::TerminalPane) (which reads it off the live
//! `alacritty_terminal` grid, history included); this module never touches alacritty types.
//! Mirrors the xterm `@xterm/addon-search` wiring in the Electron `Terminal.tsx`/`SearchBox.tsx`:
//! incremental find-as-you-type, next/prev, and a 1-based "k / n" match counter.

/// One match: its absolute grid `line` (negative = scrollback) and the `[start, end)` column
/// span on that line. Column indices are character columns (exact for ASCII, the same v1
/// caveat as `links.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: i32,
    pub start: usize,
    pub end: usize,
}

/// Search behaviour switches (the search box's Aa / .* / word toggles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// Match case exactly (default: case-insensitive).
    pub case_sensitive: bool,
    /// Interpret the query as a regular expression (regex-lite syntax).
    pub regex: bool,
    /// Whole-word matches only (boundaries = non-alphanumeric / non-`_`).
    pub whole_word: bool,
}

impl SearchOptions {
    /// Pack/unpack the bitfield the Slint search box sends: bit0 = Aa, bit1 = .*, bit2 = word.
    pub fn from_flags(flags: u32) -> Self {
        SearchOptions {
            case_sensitive: flags & 1 != 0,
            regex: flags & 2 != 0,
            whole_word: flags & 4 != 0,
        }
    }

    pub fn to_flags(self) -> u32 {
        (self.case_sensitive as u32) | ((self.regex as u32) << 1) | ((self.whole_word as u32) << 2)
    }
}

/// Diagnose a malformed regex before the shell commits a new search draft.
pub fn validate_query(query: &str, opts: SearchOptions) -> Option<String> {
    if !opts.regex {
        return None;
    }
    regex_lite::Regex::new(&regex_pattern(query, opts))
        .err()
        .map(|e| e.to_string())
}

pub(crate) fn regex_pattern(query: &str, opts: SearchOptions) -> String {
    let pattern = if opts.whole_word {
        format!(r"\b(?:{query})\b")
    } else {
        query.to_string()
    };
    if opts.case_sensitive {
        pattern
    } else {
        format!("(?i){pattern}")
    }
}

/// True when `chars[start..end]` sits on word boundaries within `hay` (the neighbours,
/// if any, must be non-word characters).
pub(crate) fn at_word_boundary(hay: &str, start_byte: usize, end_byte: usize) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let before = hay[..start_byte].chars().next_back();
    let after = hay[end_byte..].chars().next();
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

/// Find every occurrence of `query` across `lines` (behaviour from `opts`), where each
/// entry is `(absolute_line, row_text)`. Matches within a line don't overlap (the scan
/// resumes past each hit). Returns them in line order (the caller supplies lines top→bottom).
pub fn find_matches(lines: &[(i32, String)], query: &str, opts: SearchOptions) -> Vec<Match> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    if opts.regex {
        find_matches_regex(lines, query, opts, &mut out);
        return out;
    }
    // Literal path: fold case (when insensitive) and scan for non-overlapping occurrences.
    let (q, fold) = if opts.case_sensitive {
        (query.to_string(), false)
    } else {
        (query.to_lowercase(), true)
    };
    let qchars = q.chars().count();
    for (line, text) in lines {
        let hay = if fold {
            text.to_lowercase()
        } else {
            text.clone()
        };
        let mut from = 0usize;
        while from <= hay.len() {
            match hay[from..].find(&q) {
                Some(pos) => {
                    let s = from + pos;
                    let e = s + q.len();
                    if opts.whole_word && !at_word_boundary(&hay, s, e) {
                        // Advance one char so overlapping candidates aren't skipped.
                        from = s + hay[s..].chars().next().map_or(1, char::len_utf8);
                        continue;
                    }
                    let start_col = hay[..s].chars().count();
                    out.push(Match {
                        line: *line,
                        start: start_col,
                        end: start_col + qchars,
                    });
                    // Resume past this hit (q is non-empty, so this always advances).
                    from = e;
                }
                None => break,
            }
        }
    }
    out
}

/// Regex path (regex-lite): case-insensitivity via the engine, whole-word via `\b` wrap.
/// Byte offsets are converted to character columns (same caveat as the literal path).
fn find_matches_regex(
    lines: &[(i32, String)],
    query: &str,
    opts: SearchOptions,
    out: &mut Vec<Match>,
) {
    let pattern = regex_pattern(query, opts);
    let Ok(re) = regex_lite::Regex::new(&pattern) else {
        return; // invalid pattern → no matches (the counter shows 0 / No results)
    };
    for (line, text) in lines {
        let mut last_end = 0usize;
        for m in re.find_iter(text) {
            let (s, e) = (m.start(), m.end());
            if s < last_end {
                continue;
            }
            let start_col = text[..s].chars().count();
            let end_col = start_col + text[s..e].chars().count();
            out.push(Match {
                line: *line,
                start: start_col,
                end: end_col,
            });
            last_end = e;
        }
    }
}

/// Pick the index of the result to activate when search results change, biased to the first
/// match at or below `prefer_line` (so opening search jumps to the nearest match below the
/// viewport top, like xterm's `findNext`). Returns `None` for an empty set.
pub fn initial_index(matches: &[Match], prefer_line: i32) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    matches
        .iter()
        .position(|m| m.line >= prefer_line)
        .or(Some(0))
}

/// Step `idx` by `+1` (next) or `-1` (prev) with wraparound over `len` results.
pub fn step(idx: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<(i32, String)> {
        vec![
            (-2, "the quick brown fox".to_string()),
            (-1, "FOX jumps over".to_string()),
            (0, "the lazy dog".to_string()),
        ]
    }

    #[test]
    fn finds_case_insensitive_matches_with_columns() {
        let m = find_matches(&lines(), "fox", SearchOptions::default());
        assert_eq!(m.len(), 2);
        assert_eq!(
            m[0],
            Match {
                line: -2,
                start: 16,
                end: 19
            }
        );
        assert_eq!(
            m[1],
            Match {
                line: -1,
                start: 0,
                end: 3
            }
        );
    }

    #[test]
    fn finds_repeated_matches_on_one_line() {
        let m = find_matches(&[(0, "aXaXa".to_string())], "a", SearchOptions::default());
        assert_eq!(m.len(), 3);
        assert_eq!(m.iter().map(|h| h.start).collect::<Vec<_>>(), vec![0, 2, 4]);
    }

    #[test]
    fn empty_query_finds_nothing() {
        assert!(find_matches(&lines(), "", SearchOptions::default()).is_empty());
    }

    #[test]
    fn initial_index_prefers_first_match_at_or_below_a_line() {
        let m = find_matches(&lines(), "fox", SearchOptions::default());
        // prefer_line 0 → no match >= 0 → falls back to 0
        assert_eq!(initial_index(&m, 0), Some(0));
        // prefer_line -1 → second match (line -1) is the first >= -1
        assert_eq!(initial_index(&m, -1), Some(1));
        assert_eq!(initial_index(&[], 0), None);
    }

    #[test]
    fn case_sensitive_option_distinguishes_case() {
        let opts = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let m = find_matches(&lines(), "fox", opts);
        assert_eq!(m.len(), 1, "only the lowercase 'fox' on line -2");
        assert_eq!(m[0].line, -2);
    }

    #[test]
    fn whole_word_skips_substring_hits() {
        let opts = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let m = find_matches(&[(-1, "foxfox fox".to_string())], "fox", opts);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start, 7);
        // underscore and digits are word characters
        let m2 = find_matches(&[(-1, "_fox fox2".to_string())], "fox", opts);
        assert!(m2.is_empty());
    }

    #[test]
    fn regex_option_matches_patterns() {
        let opts = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let m = find_matches(&[(-1, "foo123bar".to_string())], r"\d+", opts);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].start, m[0].end), (3, 6));
        // case-insensitive stays the default for regex too
        let m2 = find_matches(&[(-1, "ABC".to_string())], "abc", opts);
        assert_eq!(m2.len(), 1);
    }

    #[test]
    fn regex_whole_word_and_case_flags_compose() {
        let opts = SearchOptions {
            regex: true,
            whole_word: true,
            case_sensitive: true,
        };
        let m = find_matches(&[(-1, "aFox Fox".to_string())], "Fox", opts);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start, 5);
    }

    #[test]
    fn invalid_regex_yields_no_matches() {
        let opts = SearchOptions {
            regex: true,
            ..Default::default()
        };
        assert!(find_matches(&lines(), "(unclosed", opts).is_empty());
    }

    #[test]
    fn flags_round_trip() {
        let o = SearchOptions {
            case_sensitive: true,
            regex: false,
            whole_word: true,
        };
        assert_eq!(SearchOptions::from_flags(o.to_flags()), o);
        assert_eq!(SearchOptions::from_flags(0), SearchOptions::default());
    }

    #[test]
    fn step_wraps_both_directions() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0); // wrap forward
        assert_eq!(step(0, 3, false), 2); // wrap backward
        assert_eq!(step(0, 0, true), 0); // empty is a no-op
    }
}
