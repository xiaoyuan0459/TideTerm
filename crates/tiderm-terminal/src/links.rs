//! Path- and URL-token extraction for clickable terminal links — the path half is a 1:1 port of
//! `src/renderer/components/pathLinks.ts`.
//!
//! Detects file-path tokens in a single rendered terminal row so the pane can turn them into
//! clickable links (resolved by the local file-links helper; see [`crate::pane`]).
//! Extraction stays pure. The host owns file/URL activation and clipboard actions.
//!
//! Also detects `http://`/`https://` URLs ([`extract_url_candidates`]) — same click UX as paths
//! (plain click opens, Ctrl-click copies), but with no on-disk verification step: a
//! well-formed URL linkifies as-is and opens in the default browser.
//!
//! Shape rule (decided): a candidate must contain a path separator OR end in a file extension.
//! Bare words like `build`/`src`/`test` never linkify even when a matching file exists — that
//! keeps prose from lighting up. The drive-letter colon in `C:\foo.ts:42` stays part of the
//! path; only a trailing `:line[:col]` is parsed off as a location suffix.
//!
//! Indices ([`PathCandidate::start`]/[`end`](PathCandidate::end)) are **character columns** into
//! the source row — exact for ASCII paths; wide (CJK) glyphs earlier on the line can shift this,
//! an accepted v1 limitation (mirrors the TS note on `cellFromIndex`).

/// A detected path-shaped token and the column range it occupies on the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCandidate {
    /// The path portion, with any `:line:col` suffix and wrapping punctuation removed.
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    /// Inclusive start column into the source row (for the link's underline range).
    pub start: usize,
    /// Exclusive end column into the source row.
    pub end: usize,
}

// Wrapping punctuation stripped from the ends of an unquoted token: brackets, backticks/quotes,
// and trailing sentence punctuation (`see src/a.ts.`).
const LEAD: &[char] = &['(', '[', '{', '<', '`', '"', '\''];
const TRAIL: &[char] = &[')', ']', '}', '>', '`', '"', '\'', ',', ';', '.', '!', '?'];

/// True when a string looks path-shaped: has a separator, or ends in an extension. Mirrors
/// `hasPathShape` (`/[\\/]/` OR `/\.[A-Za-z0-9]{1,12}$/`).
pub fn has_path_shape(p: &str) -> bool {
    if p.contains('/') || p.contains('\\') {
        return true; // separator → covers ./ ../ ~/ and C:\ too
    }
    if let Some(dot) = p.rfind('.') {
        let after = &p[dot + 1..];
        let n = after.chars().count();
        if (1..=12).contains(&n) && after.chars().all(|c| c.is_ascii_alphanumeric()) {
            return true; // trailing .ext (also catches .gitignore)
        }
    }
    false
}

/// Parse `^\d+(?::\d+)?$` — the whole string is `line` or `line:col`. Used for the unquoted
/// trailing-suffix split.
fn parse_loc(s: &str) -> Option<(u32, Option<u32>)> {
    let mut parts = s.splitn(2, ':');
    let a = parts.next()?;
    if a.is_empty() || !a.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let line: u32 = a.parse().ok()?;
    match parts.next() {
        None => Some((line, None)),
        Some(b) => {
            if b.is_empty() || !b.bytes().all(|x| x.is_ascii_digit()) {
                return None;
            }
            Some((line, Some(b.parse().ok()?)))
        }
    }
}

/// Split a trailing `:line[:col]` off a token, but only when the part before it is itself
/// path-shaped (so `localhost:3000` isn't mistaken for a located path). Mirrors `splitSuffix`:
/// the regex finds the *leftmost* colon whose tail is a pure location, then gates on
/// `hasPathShape(head)` — failing that gate yields the whole token unsplit.
fn split_suffix(core: &str) -> (String, Option<u32>, Option<u32>) {
    for (i, ch) in core.char_indices() {
        if ch != ':' || i == 0 {
            continue; // head (`.+?`) must be non-empty
        }
        let tail = &core[i + 1..];
        if let Some((line, col)) = parse_loc(tail) {
            let head = &core[..i];
            return if has_path_shape(head) {
                (head.to_string(), Some(line), col)
            } else {
                (core.to_string(), None, None)
            };
        }
    }
    (core.to_string(), None, None)
}

/// Parse a `:line[:col]` suffix appearing right after a closing quote. Returns the values plus
/// the number of characters consumed. Mirrors the quoted-path branch's
/// `/^:(\d+)(?::(\d+))?/`.
fn parse_quote_suffix(rest: &[char]) -> Option<(u32, Option<u32>, usize)> {
    if rest.first() != Some(&':') {
        return None;
    }
    let mut idx = 1;
    let line_start = idx;
    while idx < rest.len() && rest[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == line_start {
        return None; // need ≥1 digit
    }
    let line: u32 = rest[line_start..idx]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;
    let mut col = None;
    if idx < rest.len() && rest[idx] == ':' {
        let csave = idx;
        idx += 1;
        let col_start = idx;
        while idx < rest.len() && rest[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == col_start {
            idx = csave; // bare ':' with no digits → optional group doesn't match
        } else {
            col = Some(
                rest[col_start..idx]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()?,
            );
        }
    }
    Some((line, col, idx))
}

/// One token: a double-/single-quoted string (incl. its quotes), or a run of non-space,
/// non-quote characters. Returns `(token_chars, start_column)` pairs. Mirrors the TS
/// `TOKEN_RE = /"[^"]*"|'[^']*'|[^\s"']+/g` (an unterminated quote is skipped, not a token).
fn tokenize(chars: &[char]) -> Vec<(Vec<char>, usize)> {
    let mut out = Vec::new();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            // find the matching close quote
            if let Some(j) = (i + 1..n).find(|&j| chars[j] == c) {
                out.push((chars[i..=j].to_vec(), i));
                i = j + 1;
                continue;
            }
            // unterminated → the lone quote matches nothing; skip it.
            i += 1;
            continue;
        }
        // run of non-space, non-quote chars
        let start = i;
        while i < n && !chars[i].is_whitespace() && chars[i] != '"' && chars[i] != '\'' {
            i += 1;
        }
        out.push((chars[start..i].to_vec(), start));
    }
    out
}

/// Extract every path-shaped candidate from one rendered terminal row. Mirrors
/// `extractPathCandidates`.
pub fn extract_path_candidates(line: &str) -> Vec<PathCandidate> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();

    for (tok, tok_start) in tokenize(&chars) {
        let tlen = tok.len();
        let first = tok[0];

        // ---- quoted path ("C:\Program Files\app\read me.txt"), optionally with a :line:col
        // suffix right after the closing quote. ----
        if first == '"' || first == '\'' {
            let inner: String = tok[1..tlen - 1].iter().collect();
            let after = tok_start + tlen; // column just past the closing quote
            let suffix = parse_quote_suffix(&chars[after.min(chars.len())..]);
            let (line_no, col_no, end) = match suffix {
                Some((l, c, consumed)) => (Some(l), c, after + consumed),
                None => (None, None, after - 1), // exclude the closing quote from the range
            };
            if !inner.is_empty() && has_path_shape(&inner) {
                out.push(PathCandidate {
                    path: inner,
                    line: line_no,
                    col: col_no,
                    start: tok_start + 1,
                    end,
                });
            }
            continue;
        }

        // ---- unquoted run: trim wrapping punctuation, then peel the location suffix. ----
        let mut s = 0;
        let mut e = tlen;
        while s < e && LEAD.contains(&tok[s]) {
            s += 1;
        }
        while e > s && TRAIL.contains(&tok[e - 1]) {
            e -= 1;
        }
        if e <= s {
            continue;
        }
        let core: String = tok[s..e].iter().collect();
        let (path, line_no, col_no) = split_suffix(&core);
        if path.is_empty() || !has_path_shape(&path) {
            continue;
        }
        out.push(PathCandidate {
            path,
            line: line_no,
            col: col_no,
            start: tok_start + s,
            end: tok_start + e,
        });
    }

    out
}

/// A detected `http://`/`https://` URL and the column range it occupies on the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlCandidate {
    /// The URL with any trailing sentence punctuation / unbalanced closers stripped.
    pub url: String,
    /// Inclusive start column into the source row (for the link's underline range).
    pub start: usize,
    /// Exclusive end column into the source row.
    pub end: usize,
}

/// Characters that terminate a URL run. Whitespace plus delimiters that never appear raw in a
/// URL: quotes/backticks and angle brackets (`<https://…>` autolink wrapping).
fn url_stop(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// If `chars[i..]` starts with `http://` or `https://` (scheme case-insensitive), return the
/// scheme length in chars; else `None`.
fn url_scheme_len(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    for want in ['h', 't', 't', 'p'] {
        if chars.get(j).map(|c| c.to_ascii_lowercase()) != Some(want) {
            return None;
        }
        j += 1;
    }
    if chars.get(j).map(|c| c.to_ascii_lowercase()) == Some('s') {
        j += 1;
    }
    for want in [':', '/', '/'] {
        if chars.get(j) != Some(&want) {
            return None;
        }
        j += 1;
    }
    Some(j - i)
}

/// Extract every `http://`/`https://` URL from one rendered terminal row.
///
/// Boundary rules: a URL starts at a scheme not preceded by a scheme-ish char (so `xhttp://`
/// doesn't fire mid-word) and runs to whitespace/quote/angle-bracket. Trailing sentence
/// punctuation (`.`, `,`, `;`, `:`, `!`, `?`) is stripped, and trailing `)`/`]`/`}` only when
/// unbalanced within the URL — so `(https://a.com)` drops the paren but a Wikipedia-style
/// `…/Foo_(bar)` keeps it. No on-disk/network verification: shape alone linkifies.
pub fn extract_url_candidates(line: &str) -> Vec<UrlCandidate> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let Some(slen) = url_scheme_len(&chars, i) else {
            i += 1;
            continue;
        };
        // Left boundary: the scheme must not continue a word (`xhttp://`) or a longer scheme
        // (`shttp`, `foo+http`, `web.http`).
        if i > 0 {
            let prev = chars[i - 1];
            if prev.is_ascii_alphanumeric() || matches!(prev, '+' | '-' | '.') {
                i += slen;
                continue;
            }
        }
        let start = i;
        let body = start + slen; // first char after `scheme://`
        let mut e = body;
        while e < n && !url_stop(chars[e]) {
            e += 1;
        }
        // Strip trailing punctuation; closers only when unbalanced inside the URL.
        loop {
            if e <= body {
                break;
            }
            let last = chars[e - 1];
            let strip = match last {
                '.' | ',' | ';' | ':' | '!' | '?' => true,
                ')' | ']' | '}' => {
                    let open = match last {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    let opens = chars[body..e - 1].iter().filter(|&&c| c == open).count();
                    let closes = chars[body..e - 1].iter().filter(|&&c| c == last).count();
                    closes >= opens // this closer has no matching opener → wrapping punctuation
                }
                _ => false,
            };
            if !strip {
                break;
            }
            e -= 1;
        }
        // A bare scheme (`http://` then nothing, or only stripped punctuation) is not a URL.
        if e > body {
            out.push(UrlCandidate {
                url: chars[start..e].iter().collect(),
                start,
                end: e,
            });
            i = e;
        } else {
            i = body;
        }
    }
    out
}

/// Map a 0-based index into a (wrap-joined) logical line to a 1-based xterm-style cell.
/// Assumes one column per character — exact for ASCII paths. Kept for parity with the TS
/// `cellFromIndex` (the in-crate hit-tester works per single row, so it doesn't need the wrap
/// math, but downstream/test parity does).
pub fn cell_from_index(index: usize, start_row: usize, cols: usize) -> (usize, usize) {
    let cols = cols.max(1);
    ((index % cols) + 1, start_row + index / cols + 1)
}

#[cfg(test)]
mod tests;
