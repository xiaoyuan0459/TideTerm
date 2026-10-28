use super::*;

fn only(line: &str) -> Vec<PathCandidate> {
    extract_path_candidates(line)
}
fn paths(line: &str) -> Vec<String> {
    only(line).into_iter().map(|c| c.path).collect()
}
/// The substring of `line` covered by candidate `c`'s [start, end) column range.
fn slice(line: &str, c: &PathCandidate) -> String {
    line.chars().skip(c.start).take(c.end - c.start).collect()
}

// ---- hasPathShape ----
#[test]
fn has_path_shape_accepts_separators() {
    assert!(has_path_shape("src/index.ts"));
    assert!(has_path_shape("src\\index.ts"));
    assert!(has_path_shape("./build"));
    assert!(has_path_shape("../a"));
    assert!(has_path_shape("C:\\foo"));
}
#[test]
fn has_path_shape_accepts_bare_files_with_extension() {
    assert!(has_path_shape("package.json"));
    assert!(has_path_shape(".gitignore"));
}
#[test]
fn has_path_shape_rejects_bare_words() {
    assert!(!has_path_shape("build"));
    assert!(!has_path_shape("src"));
    assert!(!has_path_shape("README"));
}

// ---- extractPathCandidates ----
#[test]
fn finds_a_relative_path_and_underlines_exactly_it() {
    let line = "see src/renderer/Terminal.tsx for details";
    let c = only(line);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].path, "src/renderer/Terminal.tsx");
    assert_eq!(slice(line, &c[0]), "src/renderer/Terminal.tsx");
    assert_eq!(c[0].line, None);
}

#[test]
fn parses_line_and_line_col_suffixes() {
    let c = only("a/b.ts:42");
    assert_eq!(c[0].path, "a/b.ts");
    assert_eq!((c[0].line, c[0].col), (Some(42), None));
    let c = only("a/b.ts:42:7");
    assert_eq!(c[0].path, "a/b.ts");
    assert_eq!((c[0].line, c[0].col), (Some(42), Some(7)));
}

#[test]
fn keeps_drive_letter_colon_in_absolute_windows_path() {
    let c = only("at C:\\hyperpanes\\src\\Terminal.tsx:224");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].path, "C:\\hyperpanes\\src\\Terminal.tsx");
    assert_eq!(c[0].line, Some(224));
}

#[test]
fn handles_a_quoted_path_with_spaces() {
    let line = "open \"C:\\Program Files\\app\\read me.txt\" now";
    let c = only(line);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].path, "C:\\Program Files\\app\\read me.txt");
    // range excludes the surrounding quotes
    assert_eq!(slice(line, &c[0]), "C:\\Program Files\\app\\read me.txt");
}

#[test]
fn handles_a_quoted_path_with_suffix_after_closing_quote() {
    let c = only("\"a b.ts\":10:3");
    assert_eq!(c[0].path, "a b.ts");
    assert_eq!((c[0].line, c[0].col), (Some(10), Some(3)));
}

#[test]
fn strips_wrapping_punctuation() {
    assert_eq!(only("(src/a.ts)")[0].path, "src/a.ts");
    assert_eq!(only("`src/a.ts`")[0].path, "src/a.ts");
    assert_eq!(only("edited src/a.ts.")[0].path, "src/a.ts");
    assert_eq!(only("files: a.ts, b.ts")[0].path, "a.ts");
}

#[test]
fn does_not_mistake_host_port_or_bare_words_for_paths() {
    assert_eq!(only("listening on localhost:3000").len(), 0);
    assert_eq!(only("run the build step in src now").len(), 0);
    // shape-passes (looks like .1 ext) but the disk check downstream rejects it
    assert_eq!(paths("version v18.3.1 here"), vec!["v18.3.1"]);
}

#[test]
fn finds_multiple_paths_on_one_line() {
    assert_eq!(
        paths("moved src/a.ts -> dist/a.js"),
        vec!["src/a.ts", "dist/a.js"]
    );
}

#[test]
fn matches_dot_dotdot_and_tilde_prefixes() {
    assert_eq!(only("./scripts/run.sh")[0].path, "./scripts/run.sh");
    assert_eq!(only("../shared/x.ts")[0].path, "../shared/x.ts");
    assert_eq!(only("~/notes/todo.md")[0].path, "~/notes/todo.md");
}

// ---- extractUrlCandidates ----
fn urls(line: &str) -> Vec<String> {
    extract_url_candidates(line)
        .into_iter()
        .map(|c| c.url)
        .collect()
}
/// The substring of `line` covered by URL candidate `c`'s [start, end) column range.
fn url_slice(line: &str, c: &UrlCandidate) -> String {
    line.chars().skip(c.start).take(c.end - c.start).collect()
}

#[test]
fn finds_a_url_and_underlines_exactly_it() {
    let line = "docs at https://example.com/guide and more";
    let c = extract_url_candidates(line);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].url, "https://example.com/guide");
    assert_eq!(url_slice(line, &c[0]), "https://example.com/guide");
}

#[test]
fn finds_http_and_https_case_insensitively() {
    assert_eq!(urls("http://a.com"), vec!["http://a.com"]);
    assert_eq!(urls("HTTPS://A.COM/x"), vec!["HTTPS://A.COM/x"]);
    assert_eq!(urls("Http://mixed.example"), vec!["Http://mixed.example"]);
}

#[test]
fn strips_trailing_sentence_punctuation_from_urls() {
    assert_eq!(urls("see https://a.com/x."), vec!["https://a.com/x"]);
    assert_eq!(urls("see https://a.com/x, then"), vec!["https://a.com/x"]);
    assert_eq!(urls("https://a.com/x;"), vec!["https://a.com/x"]);
    assert_eq!(urls("really? https://a.com/x?!"), vec!["https://a.com/x"]);
    assert_eq!(urls("at https://a.com:"), vec!["https://a.com"]);
}

#[test]
fn strips_wrapping_parens_but_keeps_balanced_ones() {
    assert_eq!(urls("(https://a.com/x)"), vec!["https://a.com/x"]);
    assert_eq!(urls("[https://a.com/x]"), vec!["https://a.com/x"]);
    // A balanced paren inside the URL path is part of it (Wikipedia-style).
    assert_eq!(
        urls("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
        vec!["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
    );
    // …but a wrapping paren after a balanced pair still drops.
    assert_eq!(urls("(https://a.com/x_(y))"), vec!["https://a.com/x_(y)"]);
    assert_eq!(urls("(https://a.com/x)."), vec!["https://a.com/x"]);
}

#[test]
fn keeps_query_strings_fragments_and_ports() {
    assert_eq!(
        urls("https://a.com/search?q=rust&page=2#results"),
        vec!["https://a.com/search?q=rust&page=2#results"]
    );
    assert_eq!(
        urls("http://localhost:3000/api"),
        vec!["http://localhost:3000/api"]
    );
    // The trailing slash is part of the URL, not punctuation.
    assert_eq!(urls("https://a.com/dir/"), vec!["https://a.com/dir/"]);
}

#[test]
fn url_must_start_at_a_word_boundary() {
    assert_eq!(urls("xhttp://nope.com").len(), 0);
    assert_eq!(urls("shttps://nope.com").len(), 0);
    assert_eq!(urls("web.http://nope.com").len(), 0);
    // …but punctuation/quote boundaries are fine.
    assert_eq!(urls("<https://a.com>"), vec!["https://a.com"]);
    assert_eq!(urls("\"https://a.com\""), vec!["https://a.com"]);
    assert_eq!(urls("url=https://a.com").len(), 1);
}

#[test]
fn bare_scheme_is_not_a_url() {
    assert_eq!(urls("http://").len(), 0);
    assert_eq!(urls("https:// and nothing").len(), 0);
    assert_eq!(urls("http://.").len(), 0);
}

#[test]
fn finds_multiple_urls_on_one_line() {
    assert_eq!(
        urls("see https://a.com and http://b.org/x."),
        vec!["https://a.com", "http://b.org/x"]
    );
}

#[test]
fn ftp_and_other_schemes_do_not_linkify() {
    assert_eq!(urls("ftp://a.com file://x mailto:a@b.c").len(), 0);
}

// ---- cellFromIndex ----
#[test]
fn cell_from_index_maps_within_a_single_row() {
    assert_eq!(cell_from_index(0, 5, 80), (1, 6));
    assert_eq!(cell_from_index(79, 5, 80), (80, 6));
}
#[test]
fn cell_from_index_wraps_past_the_column_count() {
    assert_eq!(cell_from_index(80, 5, 80), (1, 7));
    assert_eq!(cell_from_index(165, 0, 80), (6, 3));
}
