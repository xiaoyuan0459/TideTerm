//! Explicit current-terminal Git metadata. Never run candidate drafts.
use crate::bridge::completion::completion_ffi::{CommandSuggestion, ShellReport};

pub(super) fn command(token: &str, dialect: u8) -> Option<String> {
    let script = match dialect {
        1 => include_str!("git_refs_bash.txt"),
        2 => include_str!("git_refs_powershell.txt"),
        _ => return None,
    };
    Some(
        script
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ")
            .replace("TOKEN", token),
    )
}

pub(super) fn decode(body: &[u8], token: &str, dialect: u8) -> Option<ShellReport> {
    let mut fields = std::str::from_utf8(body).ok()?.split(';');
    if fields.next()? != "3" || fields.next()? != token {
        return None;
    }
    let platform = match (dialect, fields.next()?) {
        (1, "b") => 1,
        (2, "w") => 2,
        (2, "p") => 3,
        _ => return None,
    };
    let error = match fields.next()? {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        _ => return None,
    };
    let repository = super::shell_report_wire::decode_text(fields.next()?, 2048)?;
    let text = super::shell_report_wire::decode_text(fields.next()?, 4096)?;
    if fields.next().is_some()
        || repository.chars().any(char::is_control)
        || text.chars().any(|c| c.is_control() && c != '\n')
    {
        return None;
    }
    let mut report = ShellReport {
        git: true,
        platform,
        git_error: error,
        repository,
        ..Default::default()
    };
    if error != 0 {
        return (report.repository.is_empty() && text.is_empty()).then_some(report);
    }
    if report.repository.is_empty() {
        return None;
    }
    let refs: Vec<_> = text.lines().collect();
    if refs.len() > 65
        || refs.iter().any(|r| !valid_ref(r))
        || refs.windows(2).any(|r| r[0] >= r[1])
    {
        return None;
    }
    report.truncated = refs.len() > 64;
    report.references = refs.into_iter().take(64).map(str::to_owned).collect();
    Some(report)
}

fn valid_ref(name: &str) -> bool {
    let Some(short) = name
        .strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("refs/tags/"))
    else {
        return false;
    };
    !short.is_empty()
        && !short.starts_with('-')
        && !name.contains("..")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        && name.split('/').all(|s| {
            !s.is_empty() && !s.starts_with('.') && !s.ends_with('.') && !s.ends_with(".lock")
        })
}

pub(super) fn supported(query: &str) -> bool {
    prefix(query).is_some()
}
fn prefix(query: &str) -> Option<(&str, &str, bool)> {
    let (start, tail, branches) = if let Some(tail) = query.strip_prefix("git switch ") {
        ("git switch -- ", tail, true)
    } else {
        ("git show ", query.strip_prefix("git show ")?, false)
    };
    if tail.len() > 128
        || !tail
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
    {
        return None;
    }
    Some((start, tail, branches))
}

pub(super) fn suggestions(report: &ShellReport, query: &str) -> Vec<CommandSuggestion> {
    if !report.git || report.state != 3 || report.git_error != 0 {
        return vec![];
    }
    let Some((start, tail, branches)) = prefix(query) else {
        return vec![];
    };
    report
        .references
        .iter()
        .filter_map(|reference| {
            let branch = reference.strip_prefix("refs/heads/");
            let short = branch.or_else(|| reference.strip_prefix("refs/tags/"))?;
            if (branches && branch.is_none())
                || !(short.starts_with(tail) || reference.starts_with(tail))
            {
                return None;
            }
            let value = if branches { short } else { reference };
            // Single-quote literal data in the exact dialect of the report. Git's
            // own revision semantics remain the user's responsibility at send time.
            let escaped = value.replace('\'', if report.platform == 1 { "'\\''" } else { "''" });
            Some(CommandSuggestion {
                kind: if branch.is_some() { 8 } else { 9 },
                label: short.into(),
                reference: reference.clone(),
                command: format!("{start}'{escaped}'"),
                detail: report.repository.clone(),
                ..Default::default()
            })
        })
        .take(50)
        .collect()
}

#[cfg(test)]
#[path = "git_refs_tests.rs"]
mod tests;
