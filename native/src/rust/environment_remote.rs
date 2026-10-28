//! SFTP namespace paths never pass through the client's filesystem path parser.
use super::{
    environment_draft::{self, Draft},
    environment_paths::finish,
    session::shared_runtime,
};
use crate::bridge::completion::completion_ffi::EnvironmentBatch;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tiderm_core::{
    protocol::Protocol,
    sftp::{DirectoryQuery, DirectoryReadError},
};

pub(super) struct Plan {
    directory: String,
    draft: Draft,
    stem: String,
    prefix: String,
}
pub(super) fn plan(cwd: &str, query: &str, dialect: u8) -> Option<Plan> {
    if !cwd.starts_with('/') || cwd.starts_with("//") || cwd.contains('\0') || cwd.len() > 4096 {
        return None;
    }
    let draft = environment_draft::parse(query, dialect, false)?;
    let token = &draft.token;
    if token.starts_with("//")
        || token.contains('\\')
        || !(token.starts_with('/') || token.starts_with("./") || token.starts_with("../"))
    {
        return None;
    }
    let at = token.rfind('/')?;
    let stem = token[..=at].to_owned();
    let directory = if token.starts_with('/') {
        stem.clone()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), stem)
    };
    if directory.len() > 4096 {
        return None;
    }
    Some(Plan {
        directory,
        prefix: token[at + 1..].into(),
        stem,
        draft,
    })
}
fn response(state: u8) -> EnvironmentBatch {
    EnvironmentBatch {
        state,
        ..Default::default()
    }
}
pub(super) fn scan(
    plan: &Plan,
    protocol: &Arc<dyn Protocol>,
    deadline: Instant,
    canceled: Arc<AtomicBool>,
) -> EnvironmentBatch {
    if canceled.load(Ordering::Relaxed) || !protocol.is_alive() {
        return response(9);
    }
    if Instant::now() >= deadline {
        return response(7);
    }
    let snapshot = shared_runtime().block_on(async {
        // Keep this future owned until its blocking work returns. Dropping a
        // timeout future would let repeated queries accumulate blocking tasks.
        let channel = protocol.open_sftp().await.map_err(|_| 10u8)?;
        let query = DirectoryQuery {
            max_entries: 2048,
            max_name_bytes: 262144,
            deadline,
            canceled: canceled.clone(),
        };
        channel
            .list_dir_bounded(&plan.directory, query)
            .await
            .map_err(|error| match error {
                DirectoryReadError::Unsupported => 10,
                DirectoryReadError::TimedOut => 7,
                DirectoryReadError::Cancelled => 9,
                _ => 6,
            })
    });
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(state) => return response(state),
    };
    let mut result = EnvironmentBatch {
        state: 2,
        truncated: snapshot.truncated,
        ..Default::default()
    };
    let mut matched = Vec::new();
    for entry in snapshot.entries {
        if canceled.load(Ordering::Relaxed) {
            return response(9);
        }
        if Instant::now() >= deadline {
            return response(7);
        }
        if entry.name.is_empty()
            || entry.name.contains(['/', '\\'])
            || !matches!(entry.permissions & 0o170000, 0o040000 | 0o100000)
        {
            result.skipped = true;
            continue;
        }
        let Some(command) = plan.draft.command(&format!(
            "{}{}{}",
            plan.stem,
            entry.name,
            if entry.is_dir { "/" } else { "" }
        )) else {
            result.skipped = true;
            continue;
        };
        if !entry.name.starts_with(&plan.prefix) {
            continue;
        }
        matched.push((entry.name, command));
    }
    finish(result, matched, 4)
}
