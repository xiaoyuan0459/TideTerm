//! Reviewed reference facts, filtered by Shell/OS. No runtime network or probes.
use super::{literal_command_name, name_match_rank, simple_command_words};
/// Explicitly known context: Shell IDs 1 Bash, 2 PowerShell, 3 cmd, 4 zsh, 5 fish, 6 sh.
/// System IDs: 0 unknown, 1 Windows, 2 Linux, 3 macOS.
#[derive(Default)]
pub struct VocabularyContext {
    pub shell_kind: u8,
    pub system: u8,
}
/// A reference fact; it makes no assertion about installed programs.
pub struct ReferenceSuggestion {
    pub command: String,
    pub label: String,
    pub description: String,
    pub detail: String,
    pub reference: String,
}
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vocabulary {
    version: u8,
    reviewed: String,
    reference: String,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    command: String,
    shells: Vec<u8>,
    systems: Vec<u8>,
    summary: String,
    source: String,
    reference_version: String,
    options: Vec<String>,
    value_options: Vec<String>,
}
fn vocabulary() -> Option<&'static Vocabulary> {
    static DATA: OnceLock<Option<Vocabulary>> = OnceLock::new();
    DATA.get_or_init(|| {
        let data: Vocabulary =
            serde_json::from_str(include_str!("../../assets/command-vocabulary.json")).ok()?;
        let mut keys = std::collections::HashSet::new();
        (data.version == 1
            && data.entries.len() <= 1024
            && data.reviewed.len() <= 32
            && data.reference.len() <= 1024
            && data.entries.iter().all(|entry| {
                literal_command_name(&entry.command)
                    && !entry.shells.is_empty()
                    && entry.shells.iter().all(|s| (1..=6).contains(s))
                    && entry.systems.iter().all(|s| (1..=3).contains(s))
                    && entry.summary.len() <= 256
                    && entry.reference_version.len() <= 256
                    && entry.source.starts_with("https://")
                    && entry.source.len() <= 512
                    && entry.options.len() <= 32
                    && entry.options.iter().all(|o| {
                        o.len() <= 64
                            && o.starts_with(['-', '/'])
                            && o.bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"-/".contains(&c))
                    })
                    && entry
                        .value_options
                        .iter()
                        .all(|o| entry.options.contains(o))
                    && keys.insert((
                        entry.command.clone(),
                        entry.shells.clone(),
                        entry.systems.clone(),
                    ))
            }))
        .then_some(data)
    })
    .as_ref()
}
pub fn suggestions(context: &VocabularyContext, query: &str) -> Vec<ReferenceSuggestion> {
    if context.shell_kind == 0 || query.trim().is_empty() || query.len() > 4096 {
        return vec![];
    }
    let Some(data) = vocabulary() else {
        return vec![];
    };
    let Some((words, trailing)) = simple_command_words(query) else {
        return vec![];
    };
    if words.is_empty() {
        return vec![];
    }
    let eq = |a: &str, b: &str| {
        if context.shell_kind == 2 || context.shell_kind == 3 {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let applicable = |entry: &&Entry| {
        entry.shells.contains(&context.shell_kind)
            && (entry.systems.is_empty() || entry.systems.contains(&context.system))
    };
    let row = |entry: &Entry, command: String| ReferenceSuggestion {
        command,
        label: entry.summary.clone(),
        description: entry.summary.clone(),
        detail: format!(
            "{}\n{}\n{}",
            entry.reference_version, data.reviewed, entry.source
        ),
        reference: entry.source.clone(),
    };
    if words.len() == 1 && !trailing {
        if !literal_command_name(query) {
            return vec![];
        }
        return data
            .entries
            .iter()
            .filter(applicable)
            .filter(|entry| {
                name_match_rank(&entry.command, query)
                    .is_some_and(|rank| rank < 2 || query.len() >= 3)
            })
            .map(|entry| row(entry, entry.command.clone()))
            .collect();
    }
    let Some(entry) = data
        .entries
        .iter()
        .filter(applicable)
        .find(|entry| eq(&entry.command, &words[0].0))
    else {
        return vec![];
    };
    let prefix = if trailing {
        ""
    } else {
        words.last().map_or("", |w| w.0.as_str())
    };
    if !prefix.is_empty() && !prefix.starts_with(['-', '/']) {
        return vec![];
    }
    let complete = if trailing {
        &words[1..]
    } else {
        &words[1..words.len() - 1]
    };
    let mut awaiting_value = false;
    for (token, _) in complete {
        if awaiting_value {
            awaiting_value = false;
            continue;
        }
        awaiting_value = entry.value_options.iter().any(|name| eq(name, token));
    }
    if awaiting_value {
        return vec![];
    }
    let start = if trailing {
        query.len()
    } else {
        words.last().map_or(query.len(), |w| w.1)
    };
    entry
        .options
        .iter()
        .filter(|option| {
            let matches = if context.shell_kind == 2 || context.shell_kind == 3 {
                option
                    .to_ascii_lowercase()
                    .starts_with(&prefix.to_ascii_lowercase())
            } else {
                option.starts_with(prefix)
            };
            matches && !complete.iter().any(|(token, _)| eq(token, option))
        })
        .map(|option| row(entry, format!("{}{option} ", &query[..start])))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_candidates_follow_shell_system_and_argument_position() {
        let query = |shell_kind, system, text| {
            suggestions(&VocabularyContext { shell_kind, system }, text)
                .into_iter()
                .map(|r| r.command)
                .collect::<Vec<_>>()
        };
        assert!(query(2, 1, "Get-Ch").contains(&"Get-ChildItem".into()));
        assert!(query(1, 2, "Get-Ch").is_empty());
        assert!(query(5, 2, "expo").is_empty());
        assert!(query(4, 2, "seto").contains(&"setopt".into()));
        assert!(query(1, 2, "seto").is_empty());
        assert!(query(1, 2, "ls").contains(&"ls".into()));
        assert!(
            query(1, 3, "ls").is_empty(),
            "GNU syntax must not be claimed on macOS"
        );
        assert!(
            query(1, 0, "ls").is_empty(),
            "unknown remote OS must not inherit local Linux"
        );
        assert!(query(0, 1, "Get-").is_empty());
        assert!(query(2, 1, "").is_empty());
        assert_eq!(
            query(2, 1, "Get-ChildItem -li"),
            ["Get-ChildItem -LiteralPath "]
        );
        assert!(query(2, 1, "Get-ChildItem -LiteralPath ").is_empty());
        for unsafe_input in [
            "Get-Content | G",
            "Get-Content $(G)",
            "Get-Content 'open",
            "Get-Content; G",
        ] {
            assert!(query(2, 1, unsafe_input).is_empty());
        }
        assert_eq!(query(5, 2, "set --ex"), ["set --export "]);
        assert!(query(2, 1, "set --ex").is_empty());
    }
}
