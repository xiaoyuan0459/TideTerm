//! Explicit Docker context metadata; never inspect/log/execute a candidate.
use crate::bridge::completion::completion_ffi::{CommandSuggestion, ContainerEntry, ShellReport};

pub(super) fn valid_context(context: &str) -> bool {
    valid_name(context, 64)
}
fn valid_name(name: &str, limit: usize) -> bool {
    !name.is_empty()
        && name.len() <= limit
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(super) fn command(token: &str, dialect: u8, context: &str) -> Option<String> {
    if !valid_context(context) {
        return None;
    }
    let template = match dialect {
        1 => include_str!("docker_snapshot_bash.txt"),
        2 => include_str!("docker_snapshot_powershell.txt"),
        _ => return None,
    };
    Some(
        template
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ")
            .replace("TOKEN", token)
            .replace("CONTEXT", context),
    )
}
pub(super) fn decode(body: &[u8], token: &str, dialect: u8, context: &str) -> Option<ShellReport> {
    let mut parts = std::str::from_utf8(body).ok()?.split(';');
    if !valid_context(context) || parts.next()? != "4" || parts.next()? != token {
        return None;
    }
    let platform = match (dialect, parts.next()?) {
        (1, "b") => 1,
        (2, "w") => 2,
        (2, "p") => 3,
        _ => return None,
    };
    let error = match parts.next()? {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        _ => return None,
    };
    if parts.next()? != context {
        return None;
    }
    let text = super::shell_report_wire::decode_text(parts.next()?, 6144)?;
    if parts.next().is_some() {
        return None;
    }
    let mut result = ShellReport {
        docker_context: context.into(),
        docker_error: error,
        platform,
        ..Default::default()
    };
    if error != 0 {
        return text.is_empty().then_some(result);
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return None;
    }
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for row in text.lines() {
        let (id_json, name_json) = row.split_once('|')?;
        let id: String = serde_json::from_str(id_json).ok()?;
        let name: String = serde_json::from_str(name_json).ok()?;
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !valid_name(&name, 128)
            || !ids.insert(id.clone())
            || !names.insert(name.clone())
        {
            return None;
        }
        if result.containers.len() == 33 {
            return None;
        }
        result.containers.push(ContainerEntry { id, name });
    }
    result.truncated = result.containers.len() > 32;
    result.containers.truncate(32);
    Some(result)
}
fn prefix(query: &str) -> Option<(&str, &str)> {
    let (verb, tail) = if let Some(tail) = query.strip_prefix("docker inspect ") {
        ("inspect", tail)
    } else {
        ("logs", query.strip_prefix("docker logs ")?)
    };
    (tail.len() <= 128
        && tail
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
    .then_some((verb, tail))
}
pub(super) fn supported(query: &str) -> bool {
    prefix(query).is_some()
}
pub(super) fn suggestions(report: &ShellReport, query: &str) -> Vec<CommandSuggestion> {
    let Some((verb, tail)) = prefix(query) else {
        return vec![];
    };
    if report.state != 3 || report.docker_error != 0 || !valid_context(&report.docker_context) {
        return vec![];
    }
    report
        .containers
        .iter()
        .filter(|c| c.name.starts_with(tail) || c.id.starts_with(tail))
        .map(|c| CommandSuggestion {
            kind: 10,
            label: c.name.clone(),
            reference: c.id.clone(),
            detail: report.docker_context.clone(),
            command: format!(
                "docker --context '{}' container {verb} '{}'",
                report.docker_context, c.id
            ),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
#[path = "docker_snapshot_tests.rs"]
mod tests;
