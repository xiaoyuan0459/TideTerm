//! Explicit scoped resource metadata; credential-capable sending is separately gated.
use crate::bridge::completion::completion_ffi::{
    CommandSuggestion, KubeEntry, KubeScope, ShellReport,
};
pub(super) fn valid_scope(s: &KubeScope, _dialect: u8) -> bool {
    let c = &s.config;
    let absolute = c.starts_with('/')
        || (c.len() > 3
            && c.as_bytes()[0].is_ascii_alphabetic()
            && c.as_bytes()[1] == b':'
            && matches!(c.as_bytes()[2], b'/' | b'\\'));
    resource_name(s.resource).is_some()
        && absolute
        && c.len() <= 512
        && !c.starts_with("//")
        && !c.chars().any(|c| c.is_control() || "\"‘’“”".contains(c))
        && !s.context.is_empty()
        && s.context.len() <= 128
        && s.context
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:@+-".contains(&b))
        && label(&s.namespace_name, 63)
}
fn label(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
}
fn pod_name(s: &str) -> bool {
    s.len() <= 253 && s.split('.').all(|s| label(s, 63))
}
pub(super) fn scope_hex(s: &KubeScope) -> String {
    hex(
        serde_json::json!({"config":s.config,"context":s.context,"namespace":s.namespace_name,"resource":s.resource})
            .to_string()
            .as_bytes(),
    )
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(super) fn command(token: &str, dialect: u8, s: &KubeScope) -> Option<String> {
    if !valid_scope(s, dialect) {
        return None;
    }
    let template = match dialect {
        1 => include_str!("kube_snapshot_bash.txt"),
        2 => include_str!("kube_snapshot_powershell.txt"),
        _ => return None,
    };
    let encode = |v: &str| {
        if dialect == 1 {
            v.as_bytes().iter().map(|b| format!("\\x{b:02x}")).collect()
        } else {
            hex(v.as_bytes())
        }
    };
    Some(
        template
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ")
            .replace("TOKEN", token)
            .replace("SCOPEHEX", &scope_hex(s))
            .replace("CFGHEX", &encode(&s.config))
            .replace("CTXHEX", &encode(&s.context))
            .replace("NSHEX", &encode(&s.namespace_name))
            .replace("RESOURCE", &format!("{}s", resource_name(s.resource)?)),
    )
}
pub(super) fn decode(body: &[u8], token: &str, dialect: u8, s: &KubeScope) -> Option<ShellReport> {
    resource_name(s.resource)?;
    let mut p = std::str::from_utf8(body).ok()?.split(';');
    if p.next()? != "5" || p.next() != Some(token) {
        return None;
    }
    let platform = match (dialect, p.next()?) {
        (1, "b") => 1,
        (2, "w") => 2,
        (2, "p") => 3,
        _ => return None,
    };
    let error = match p.next()? {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        _ => return None,
    };
    if p.next()? != scope_hex(s) {
        return None;
    }
    let text = super::shell_report_wire::decode_text(p.next()?, 6144)?;
    if p.next().is_some() {
        return None;
    }
    let mut r = ShellReport {
        kube: s.clone(),
        platform,
        kube_error: error,
        ..Default::default()
    };
    if error != 0 {
        return text.is_empty().then_some(r);
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return None;
    }
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for row in text.lines() {
        let fields: Vec<_> = row.split('|').collect();
        if fields.len() != 3 {
            return None;
        }
        let uid: String = serde_json::from_str(fields[0]).ok()?;
        let name: String = serde_json::from_str(fields[1]).ok()?;
        let ns: String = serde_json::from_str(fields[2]).ok()?;
        if uid.is_empty()
            || uid.len() > 128
            || !uid
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || !pod_name(&name)
            || ns != s.namespace_name
            || !ids.insert(uid.clone())
            || !names.insert(name.clone())
            || r.resources.len() == 33
        {
            return None;
        }
        r.resources.push(KubeEntry { uid, name });
    }
    r.truncated = r.resources.len() > 32;
    r.resources.truncate(32);
    Some(r)
}
pub(super) fn resource_name(resource: u8) -> Option<&'static str> {
    match resource {
        0 => Some("pod"),
        1 => Some("service"),
        2 => Some("deployment"),
        _ => None,
    }
}
fn prefix(q: &str) -> Option<(&str, u8, &str)> {
    let q = q.strip_prefix("kubectl ")?;
    let (v, q) = q.split_once(' ')?;
    if !matches!(v, "get" | "describe") {
        return None;
    }
    let (resource, p) = q.split_once(' ')?;
    let resource = match resource {
        "pod" | "pods" | "po" => 0,
        "service" | "services" | "svc" => 1,
        "deployment" | "deployments" | "deploy" => 2,
        _ => return None,
    };
    (p.len() <= 253
        && p.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b)))
    .then_some((v, resource, p))
}
pub(super) fn provider(q: &str) -> Option<u8> {
    prefix(q).map(|(_, r, _)| 4 + r)
}
pub(super) fn suggestions(r: &ShellReport, q: &str) -> Vec<CommandSuggestion> {
    let Some((verb, resource, prefix)) = prefix(q) else {
        return vec![];
    };
    if r.state != 3 || r.kube.config.is_empty() || r.kube_error != 0 || r.kube.resource != resource
    {
        return vec![];
    }
    let quote = |s: &str| {
        format!(
            "'{}'",
            s.replace('\'', if r.platform == 1 { "'\\''" } else { "''" })
        )
    };
    r.resources
        .iter()
        .filter(|p| p.name.starts_with(prefix))
        .map(|p| CommandSuggestion {
            kind: 11,
            label: p.name.clone(),
            reference: p.uid.clone(),
            detail: format!(
                "{}\n{} / {}",
                r.kube.config, r.kube.context, r.kube.namespace_name
            ),
            command: format!(
                "{}kubectl {} {} {} {verb} {} {}",
                if r.platform == 1 {
                    "MSYS2_ARG_CONV_EXCL='*' "
                } else {
                    ""
                },
                quote(&format!("--kubeconfig={}", r.kube.config)),
                quote(&format!("--context={}", r.kube.context)),
                quote(&format!("--namespace={}", r.kube.namespace_name)),
                resource_name(resource).unwrap_or_default(),
                quote(&p.name)
            ),
            ..Default::default()
        })
        .collect()
}
#[cfg(test)]
#[path = "kube_snapshot_tests.rs"]
mod tests;
