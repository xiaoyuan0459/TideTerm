//! Reviewed, embedded CLI rules. They describe syntax, never probe a session environment.
use crate::bridge::completion::completion_ffi::CommandSuggestion;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    version: u8,
    reviewed: String,
    families: Vec<Family>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Family {
    command: String,
    source: String,
    platform: String,
    reference: String,
    rules: Vec<Rule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    path: String,
    description: String,
    source: String,
    argument: String,
    options: Vec<OptionRule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionRule {
    token: String,
    argument: String,
}

fn pack() -> Result<&'static Pack, ()> {
    static PACK: OnceLock<Result<Pack, ()>> = OnceLock::new();
    PACK.get_or_init(|| {
        let p: Pack = serde_json::from_str(include_str!("../../assets/completion-rules.json"))
            .map_err(|_| ())?;
        if p.version != 1
            || p.families.len() > 16
            || p.families.iter().any(|f| {
                f.rules.len() > 64
                    || f.command.is_empty()
                    || !f.source.starts_with("https://")
                    || f.platform.is_empty()
                    || f.reference.is_empty()
                    || f.rules.iter().any(|r| {
                        r.path.is_empty()
                            || !r.source.starts_with("https://")
                            || r.options.len() > 32
                    })
            })
        {
            return Err(());
        }
        Ok(p)
    })
    .as_ref()
    .map_err(|_| ())
}

// Only complete a simple command at its end. Refuse unfinished quotes and shell operators.
use tiderm_core::completion::simple_command_words as words;

fn suggestion(
    f: &Family,
    rule: Option<&Rule>,
    command: String,
    argument: &str,
    p: &Pack,
) -> CommandSuggestion {
    let source = rule.map_or(f.source.as_str(), |r| r.source.as_str());
    CommandSuggestion {
        kind: 2,
        command,
        label: argument.to_owned(),
        description: rule.map_or("", |r| r.description.as_str()).into(),
        detail: format!(
            "{}\n{}\n{}\n{}",
            f.platform, f.reference, p.reviewed, source
        ),
        ..Default::default()
    }
}
pub(super) fn suggest(query: &str) -> Result<Vec<CommandSuggestion>, ()> {
    let p = pack()?;
    let Some((mut words, trailing)) = words(query) else {
        return Ok(vec![]);
    };
    let (prefix, start) = if trailing {
        (String::new(), query.len())
    } else {
        words.pop().unwrap_or_default()
    };
    if query[start..] != prefix {
        return Ok(vec![]);
    }
    let base = &query[..start];
    let tokens: Vec<_> = words.into_iter().map(|(word, _)| word).collect();
    let mut out = Vec::new();
    if tokens.is_empty() {
        // A syntax pack is not command discovery. Only complete arguments of
        // a command the user has already entered, never advertise installed tools.
        return Ok(out);
    }
    let Some(f) = p.families.iter().find(|f| f.command == tokens[0]) else {
        return Ok(out);
    };
    // Resolve path tokens before options/arguments. Never treat an option value as another option.
    let path_tokens = &tokens[1..];
    let rule = f
        .rules
        .iter()
        .filter(|r| {
            let path: Vec<_> = r.path.split_whitespace().collect();
            path_tokens.len() >= path.len() && path.iter().zip(path_tokens).all(|(a, b)| *a == b)
        })
        .max_by_key(|r| r.path.split_whitespace().count());
    if let Some(rule) = rule {
        let skip = rule.path.split_whitespace().count();
        let args = &path_tokens[skip..];
        let mut needs_value = false;
        for arg in args {
            if needs_value {
                needs_value = false;
                continue;
            }
            if arg.starts_with('-')
                && !rule
                    .options
                    .iter()
                    .any(|o| arg == &o.token || arg.starts_with(&(o.token.clone() + "=")))
            {
                return Ok(vec![]);
            }
            needs_value = rule
                .options
                .iter()
                .any(|o| o.token == *arg && !o.argument.is_empty());
        }
        if !needs_value && (prefix.is_empty() || prefix.starts_with('-')) {
            for option in &rule.options {
                if option.token.starts_with(&prefix)
                    && !args
                        .iter()
                        .any(|s| s == &option.token || s.starts_with(&(option.token.clone() + "=")))
                {
                    out.push(suggestion(
                        f,
                        Some(rule),
                        format!("{base}{} ", option.token),
                        &option.argument,
                        p,
                    ));
                }
            }
        }
    } else {
        let mut seen = std::collections::HashSet::new();
        for r in &f.rules {
            let path: Vec<_> = r.path.split_whitespace().collect();
            if path.len() > path_tokens.len() && path.iter().zip(path_tokens).all(|(a, b)| *a == b)
            {
                let token = path[path_tokens.len()];
                if token.starts_with(&prefix) && seen.insert(token) {
                    out.push(suggestion(
                        f,
                        Some(r),
                        format!("{base}{token} "),
                        &r.argument,
                        p,
                    ));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_candidates_follow_command_structure_and_do_not_guess_dynamic_values_or_shell_fragments()
    {
        let commands = |q| {
            suggest(q)
                .unwrap()
                .into_iter()
                .map(|s| s.command)
                .collect::<Vec<_>>()
        };
        for draft in ["", " ", "do", "git", "kub"] {
            assert!(
                commands(draft).is_empty(),
                "unconfirmed command discovery: {draft}"
            );
        }
        assert_eq!(commands("docker cont"), ["docker container "]);
        assert_eq!(commands("docker container lo"), ["docker container logs "]);
        assert!(commands("git log --o").contains(&"git log --oneline ".into()));
        assert_eq!(commands("docker logs --ta"), ["docker logs --tail "]);
        for text in [
            "docker logs --tail ",
            "docker logs --unknown ",
            "git log \"$(pwd)\" ",
            "docker logs mycont",
            "git status; do",
            "echo x | gi",
            "git log \"open",
            "git log \"--o\"",
            "\"中文\"",
            "git log foo\\ ",
            "$(gi",
            "unknown ",
        ] {
            assert!(commands(text).is_empty(), "{text}");
        }
        assert!(!commands("git log --oneline ").contains(&"git log --oneline --oneline ".into()));
        let row = &suggest("kubectl get --out").unwrap()[0];
        assert_eq!(row.label, "format");
        assert!(row.detail.contains("https://kubernetes.io/") && row.detail.contains("2026-10-17"));
    }
}
