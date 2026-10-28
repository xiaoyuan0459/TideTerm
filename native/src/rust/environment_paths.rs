//! One directory snapshot, without invoking a shell or reading file contents.
use super::environment_draft::{self, Draft};
use crate::bridge::completion::completion_ffi::{CommandSuggestion, EnvironmentBatch};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone)]
pub(super) struct Plan {
    directory: PathBuf,
    draft: Draft,
    stem: String,
    prefix: String,
    separator: char,
}

pub(super) fn plan(cwd: &str, query: &str, dialect: u8) -> Option<Plan> {
    let draft = environment_draft::parse(query, dialect, cfg!(windows))?;
    let token = &draft.token;
    if token.is_empty()
        || ["//", "\\\\"]
            .iter()
            .any(|p| token.starts_with(p) || cwd.starts_with(p))
    {
        return None;
    }
    let absolute = Path::new(token).is_absolute();
    if !absolute
        && !["./", "../", ".\\", "..\\"]
            .iter()
            .any(|p| token.starts_with(p))
    {
        return None;
    }
    let at = token.rfind(|c| c == '/' || (cfg!(windows) && c == '\\'))?;
    let stem = &token[..=at];
    let directory = if absolute {
        PathBuf::from(stem)
    } else {
        Path::new(cwd).join(stem)
    };
    if !directory.is_absolute() {
        return None;
    }
    Some(Plan {
        directory,
        stem: stem.to_owned(),
        prefix: token[at + 1..].to_owned(),
        separator: if cfg!(windows) && stem.contains('\\') {
            '\\'
        } else {
            '/'
        },
        draft,
    })
}

pub(super) fn scan(plan: &Plan, deadline: Instant, current: impl Fn() -> bool) -> EnvironmentBatch {
    let mut result = EnvironmentBatch {
        state: 2,
        ..Default::default()
    };
    let running = || current() && Instant::now() < deadline;
    if !running() {
        result.state = 7;
        return result;
    }
    let Ok(entries) = std::fs::read_dir(&plan.directory) else {
        result.state = 6;
        return result;
    };
    let mut matched = Vec::new();
    let prefix = if cfg!(windows) {
        plan.prefix.to_lowercase()
    } else {
        plan.prefix.clone()
    };
    for (count, entry) in entries.enumerate() {
        if !running() {
            return EnvironmentBatch {
                state: 7,
                ..Default::default()
            };
        }
        if count == 2048 {
            result.truncated = true;
            break;
        }
        let Ok(entry) = entry else {
            result.state = 6;
            result.items.clear();
            return result;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            result.skipped = true;
            continue;
        };
        let key = if cfg!(windows) {
            name.to_lowercase()
        } else {
            name.to_owned()
        };
        let Ok(kind) = entry.file_type() else {
            result.skipped = true;
            continue;
        };
        if !kind.is_file() && !kind.is_dir() {
            result.skipped = true;
            continue;
        }
        let suffix = if kind.is_dir() {
            plan.separator.to_string()
        } else {
            String::new()
        };
        let Some(command) = plan
            .draft
            .command(&format!("{}{}{}", plan.stem, name, suffix))
        else {
            result.skipped = true;
            continue;
        };
        if !key.starts_with(&prefix) {
            continue;
        }
        matched.push((key, command));
    }
    if !running() {
        return EnvironmentBatch {
            state: 7,
            ..Default::default()
        };
    }
    finish(result, matched, 3)
}

pub(super) fn finish(
    mut result: EnvironmentBatch,
    mut matched: Vec<(String, String)>,
    kind: u8,
) -> EnvironmentBatch {
    matched.sort();
    let mut bytes = 0;
    for (_, command) in matched {
        bytes += command.len();
        if result.items.len() == 50 || bytes > 65536 {
            result.truncated = true;
            break;
        }
        result.items.push(CommandSuggestion {
            kind,
            command,
            ..Default::default()
        });
    }
    result
}
