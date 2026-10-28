//! An explicit, bounded catalog of names from the current interactive Shell.
use super::shell_report_wire::decode_text;
use crate::bridge::completion::completion_ffi::{CommandName, CommandSuggestion, ShellReport};

pub(super) fn valid_prefix(prefix: &str) -> bool {
    prefix.len() <= 64
        && prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(super) fn command(token: &str, dialect: u8, prefix: &str) -> Option<String> {
    if !valid_prefix(prefix) {
        return None;
    }
    let source = match dialect {
        1 => include_str!("shell_names_bash.txt"),
        2 => include_str!("shell_names_powershell.txt"),
        _ => return None,
    };
    Some(
        source
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ")
            .replace("__TOKEN__", token)
            .replace("__PREFIX__", prefix),
    )
}
pub(super) fn decode(body: &[u8], token: &str, dialect: u8, prefix: &str) -> Option<ShellReport> {
    let mut parts = std::str::from_utf8(body).ok()?.splitn(6, ';');
    if parts.next()? != "N" || parts.next()? != token {
        return None;
    }
    let (platform, system) = match (dialect, parts.next()?) {
        (1, "b") => (1, 0),
        (1, "b-w") => (1, 1),
        (1, "b-l") => (1, 2),
        (1, "b-m") => (1, 3),
        (2, "w") => (2, 1),
        (2, "p") => (3, 0),
        (2, "p-l") => (3, 2),
        (2, "p-m") => (3, 3),
        _ => return None,
    };
    let error = match parts.next()? {
        "0" => 0,
        "1" => 1,
        _ => return None,
    };
    let mut truncated = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let payload = parts.next()?;
    if error != 0 && !payload.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    let mut bytes = 0;
    let mut seen = std::collections::HashSet::new();
    for (count, record) in payload.split(',').filter(|s| !s.is_empty()).enumerate() {
        if count >= 256 {
            return None;
        }
        let (kind, hex) = record.split_once(':')?;
        let kind = kind.parse::<u8>().ok().filter(|v| (1..=9).contains(v))?;
        bytes += hex.len() / 2;
        if bytes > 6144 {
            return None;
        }
        let Some(name) = decode_text(hex, 128) else {
            truncated = true;
            continue;
        };
        if !super::shell_resolution::valid_name(&name)
            || !name
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
        {
            truncated = true;
            continue;
        }
        let key = if platform == 1 {
            name.clone()
        } else {
            name.to_ascii_lowercase()
        };
        if seen.insert(key) {
            names.push(CommandName { name, kind });
        }
    }
    Some(ShellReport {
        system,
        name_catalog: true,
        name_prefix: prefix.into(),
        name_error: error,
        names,
        platform,
        truncated,
        ..Default::default()
    })
}
pub(super) use tiderm_core::completion::name_match_rank as score;
pub(super) fn suggestions(report: &ShellReport, query: &str) -> Vec<CommandSuggestion> {
    if !report.name_catalog
        || report.state != 3
        || (!query.is_empty() && !super::shell_resolution::valid_name(query))
    {
        return Vec::new();
    }
    let mut rows: Vec<_> = report
        .names
        .iter()
        .filter(|n| super::shell_resolution::accepts(n.kind))
        .filter_map(|n| score(&n.name, query).map(|rank| (rank, n)))
        .collect();
    rows.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.name.cmp(&y.name)));
    rows.into_iter()
        .take(50)
        .map(|(_, n)| CommandSuggestion {
            kind: 12,
            command: n.name.clone(),
            reference: n.name.clone(),
            detail: n.kind.to_string(),
            ..Default::default()
        })
        .collect()
}
