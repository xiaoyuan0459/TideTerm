//! Match quality precedes local usage. This model owns no user storage or IO.
use std::collections::HashSet;

/// Provenance affects confidence; an explicitly requested exact lookup stays visible.
#[derive(Clone, Copy)]
pub enum CandidateSource {
    History,
    User,
    Syntax,
    Environment,
    ExactLookup,
    Names,
    Resource,
    Reference,
}
/// Facts derived only from the current terminal's submitted commands.
#[derive(Clone, Copy, Default)]
pub struct UsageWeight {
    pub count: u32,
    pub recent: usize,
    pub same_directory: bool,
}
/// Borrowed candidate facts. Environment/syntax producers have already matched tokens.
pub struct RankCandidate<'a> {
    pub command: &'a str,
    pub label: &'a str,
    pub is_template: bool,
    pub source: CandidateSource,
    pub usage: UsageWeight,
}
/// Prefix, containment and then subsequence, in that order.
pub fn name_match_rank(name: &str, query: &str) -> Option<usize> {
    let name = name.to_lowercase();
    let query = query.to_lowercase();
    if name.starts_with(&query) {
        return Some(0);
    }
    if name.contains(&query) {
        return Some(1);
    }
    let mut wanted = query.chars();
    let mut next = wanted.next();
    for character in name.chars() {
        if next == Some(character) {
            next = wanted.next();
        }
    }
    next.is_none().then_some(2)
}
/// Return at most 50 distinct candidate indexes, keeping templates as separate actions.
pub fn rank_indices(candidates: &[RankCandidate<'_>], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut matches: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let command = item.command.to_lowercase();
            let label = item.label.to_lowercase();
            let tier = if matches!(item.source, CandidateSource::Names)
                || (matches!(item.source, CandidateSource::Reference)
                    && !query.contains(char::is_whitespace))
            {
                name_match_rank(item.command, &query)?
            } else if matches!(
                item.source,
                CandidateSource::Environment
                    | CandidateSource::ExactLookup
                    | CandidateSource::Resource
            ) || command.starts_with(&query)
            {
                0
            } else if matches!(item.source, CandidateSource::Syntax)
                || command.contains(&query)
                || label.contains(&query)
            {
                1
            } else if query.len() >= 3
                && query.len() <= 64
                && !query.contains(char::is_whitespace)
                && command.len() <= 1024
            {
                name_match_rank(command.split_whitespace().next().unwrap_or(""), &query)?
            } else {
                return None;
            };
            let base = match item.source {
                CandidateSource::History => 220,
                CandidateSource::User => 160,
                CandidateSource::Syntax => 30,
                CandidateSource::Environment => 185,
                CandidateSource::ExactLookup => 600,
                CandidateSource::Names | CandidateSource::Resource => 240,
                CandidateSource::Reference => 40,
            };
            let use_weight = item.usage;
            let learned = if use_weight.count == 0 {
                0
            } else {
                (31 - use_weight.count.leading_zeros()).min(7) * 16 + 80
                    - use_weight.recent.min(80) as u32
                    + if use_weight.same_directory { 80 } else { 0 }
            };
            Some((tier, base + learned, index))
        })
        .collect();
    matches.sort_by(|(a, aw, _), (b, bw, _)| a.cmp(b).then_with(|| bw.cmp(aw)));
    let mut seen = HashSet::new();
    matches
        .into_iter()
        .filter_map(|(_, _, index)| {
            let item = &candidates[index];
            seen.insert((item.command, item.is_template))
                .then_some(index)
        })
        .take(50)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranking_keeps_match_quality_then_learns_usage_and_preserves_exact_requests() {
        let item = |command, source, usage| RankCandidate {
            command,
            label: "",
            is_template: false,
            source,
            usage,
        };
        let entries = [
            item(
                "git status",
                CandidateSource::History,
                UsageWeight {
                    count: 8,
                    recent: 8,
                    same_directory: false,
                },
            ),
            item(
                "git show",
                CandidateSource::History,
                UsageWeight {
                    count: 1,
                    recent: 0,
                    same_directory: false,
                },
            ),
            item(
                "git remote",
                CandidateSource::Reference,
                UsageWeight::default(),
            ),
            item(
                "echo git",
                CandidateSource::History,
                UsageWeight {
                    count: 500,
                    recent: 0,
                    same_directory: true,
                },
            ),
        ];
        assert_eq!(rank_indices(&entries, "git"), [0, 1, 2, 3]);
        let entries = [
            item(
                "git status",
                CandidateSource::History,
                UsageWeight {
                    count: 8,
                    recent: 8,
                    same_directory: false,
                },
            ),
            item(
                "git show",
                CandidateSource::History,
                UsageWeight {
                    count: 1,
                    recent: 0,
                    same_directory: true,
                },
            ),
            item("git", CandidateSource::ExactLookup, UsageWeight::default()),
        ];
        assert_eq!(rank_indices(&entries, "git"), [2, 1, 0]);
        assert_eq!(name_match_rank("Get-ChildItem", "gci"), Some(2));
        let entries = [
            item(
                "Get-Content file-with-i",
                CandidateSource::History,
                UsageWeight {
                    count: 500,
                    recent: 0,
                    same_directory: true,
                },
            ),
            item(
                "Get-ChildItem",
                CandidateSource::Reference,
                UsageWeight::default(),
            ),
        ];
        assert_eq!(
            rank_indices(&entries, "gci"),
            [1],
            "an argument cannot supply letters missing from the command name"
        );
        assert!(
            rank_indices(&entries, "gci file").is_empty(),
            "whole command lines are not loose subsequences"
        );
    }
}
