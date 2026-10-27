//! Explicit directory metadata, never an implicit PATH or command-resolution probe.
use super::{command_completion::completion_valid, store::SessionKind, term::try_with_tab};
use crate::bridge::completion::completion_ffi::{
    CompletionContext, EnvironmentBatch, ProgramSource,
};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;
use tiderm_core::protocol::Protocol;

pub(super) struct Plan {
    pub directories: Vec<String>,
    pub prefix: String,
    pub dialect: u8,
    pub protocol: Option<Arc<dyn Protocol>>,
}
pub(super) fn prefix(query: &str) -> bool {
    query.len() <= 128
        && query
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}
pub(super) fn directories(dirs: &[String], remote: bool) -> Result<Vec<String>, u8> {
    if dirs.is_empty() {
        return Err(12);
    }
    if dirs.len() > 8 || dirs.iter().map(String::len).sum::<usize>() > 16384 {
        return Err(13);
    }
    let mut result = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for dir in dirs {
        if dir.is_empty()
            || dir.len() > 4096
            || dir.chars().any(char::is_control)
            || dir.starts_with("//")
            || dir.starts_with("\\\\")
            || if remote {
                !dir.starts_with('/') || dir.contains('\\')
            } else {
                !std::path::Path::new(dir).is_absolute()
            }
        {
            return Err(13);
        }
        let key = if !remote && cfg!(windows) {
            dir.to_lowercase()
        } else {
            dir.clone()
        };
        if keys.insert(
            key.trim_end_matches(|c| c == '/' || (!remote && cfg!(windows) && c == '\\'))
                .to_owned(),
        ) {
            result.push(dir.clone());
        }
    }
    Ok(result)
}
fn source_kind(source: u32) -> Result<bool, u8> {
    try_with_tab(source, |s| match s.config.kind {
        SessionKind::Local => Ok(false),
        SessionKind::Ssh if s.config.meta.sftp_enabled => Ok(true),
        SessionKind::Ssh => Err(11),
        _ => Err(3),
    })
    .unwrap_or(Err(9))
}
pub fn programs_validate(context: &CompletionContext, settings: &ProgramSource) -> u8 {
    if !completion_valid(context) {
        return 9;
    }
    let remote = match source_kind(context.source) {
        Ok(remote) => remote,
        Err(state) => return state,
    };
    if !settings.configured {
        return 12;
    }
    if settings.source != context.source || settings.generation != context.generation {
        return 14;
    }
    directories(&settings.directories, remote)
        .err()
        .unwrap_or(0)
}
impl Plan {
    pub(super) fn capture(
        context: &CompletionContext,
        query: &str,
        dialect: u8,
        settings: &ProgramSource,
    ) -> Result<Self, u8> {
        if dialect > 2 {
            return Err(5);
        }
        let state = programs_validate(context, settings);
        if state != 0 {
            return Err(state);
        }
        let remote = source_kind(context.source)?;
        let protocol = if remote {
            Some(
                try_with_tab(context.source, |s| s.host.protocol())
                    .flatten()
                    .ok_or(9)?,
            )
        } else {
            None
        };
        Ok(Self {
            directories: directories(&settings.directories, remote)?,
            prefix: query.into(),
            dialect,
            protocol,
        })
    }
    pub(super) fn remote(&self) -> bool {
        self.protocol.is_some()
    }
    pub(super) fn scan(&self, deadline: Instant, canceled: Arc<AtomicBool>) -> EnvironmentBatch {
        super::program_scan::scan(self, deadline, canceled)
    }
}

#[cfg(test)]
#[path = "program_source_tests.rs"]
mod tests;
