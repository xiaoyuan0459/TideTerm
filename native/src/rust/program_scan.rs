use super::{environment_draft::invocation, program_source::Plan, session::shared_runtime};
use crate::bridge::completion::completion_ffi::{CommandSuggestion, EnvironmentBatch};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tiderm_core::sftp::{DirectoryQuery, DirectoryReadError, SftpFileEntry};

fn response(state: u8) -> EnvironmentBatch {
    EnvironmentBatch {
        state,
        ..Default::default()
    }
}
fn check(deadline: Instant, canceled: &AtomicBool) -> Result<(), u8> {
    if canceled.load(Ordering::Relaxed) {
        Err(9)
    } else if Instant::now() >= deadline {
        Err(7)
    } else {
        Ok(())
    }
}
pub(super) fn scan(plan: &Plan, deadline: Instant, canceled: Arc<AtomicBool>) -> EnvironmentBatch {
    match collect(plan, deadline, &canceled) {
        Ok(result) => result,
        Err(state) => response(state),
    }
}
fn collect(
    plan: &Plan,
    deadline: Instant,
    canceled: &Arc<AtomicBool>,
) -> Result<EnvironmentBatch, u8> {
    check(deadline, canceled)?;
    let channel = if let Some(protocol) = &plan.protocol {
        if !protocol.is_alive() {
            return Err(9);
        }
        Some(
            shared_runtime()
                .block_on(protocol.open_sftp())
                .map_err(|_| 10u8)?,
        )
    } else {
        None
    };
    let mut result = response(2);
    let mut candidates = std::collections::BTreeMap::new();
    let quota = 2048 / plan.directories.len();
    for directory in &plan.directories {
        check(deadline, canceled)?;
        let entries = if let Some(channel) = &channel {
            let query = DirectoryQuery {
                max_entries: quota,
                max_name_bytes: 262144 / plan.directories.len(),
                deadline,
                canceled: canceled.clone(),
            };
            let snapshot = shared_runtime()
                .block_on(channel.list_dir_bounded(directory, query))
                .map_err(|e| match e {
                    DirectoryReadError::TimedOut => 7u8,
                    DirectoryReadError::Cancelled => 9,
                    DirectoryReadError::Unsupported => 10,
                    _ => 6,
                })?;
            result.truncated |= snapshot.truncated;
            snapshot.entries
        } else {
            let mut entries = Vec::new();
            let mut bytes = 0;
            let reader = std::fs::read_dir(directory).map_err(|_| 6u8)?;
            for (index, entry) in reader.enumerate() {
                check(deadline, canceled)?;
                if index == quota {
                    result.truncated = true;
                    break;
                }
                let entry = entry.map_err(|_| 6u8)?;
                if !entry.file_type().map_err(|_| 6u8)?.is_file() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    result.skipped = true;
                    continue;
                };
                let data = entry.metadata().map_err(|_| 6u8)?;
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    data.permissions().mode()
                };
                #[cfg(not(unix))]
                let permissions = 0o100755;
                let path = entry.path().to_str().map(str::to_owned).ok_or(6u8)?;
                bytes += name.len() + path.len();
                if bytes > 262144 / plan.directories.len() {
                    result.truncated = true;
                    break;
                }
                let _ = data;
                entries.push(SftpFileEntry {
                    name,
                    path,
                    is_dir: false,
                    size_bytes: 0,
                    modified_unix: 0,
                    permissions,
                    owner: None,
                    group: None,
                });
            }
            entries
        };
        for entry in entries {
            check(deadline, canceled)?;
            if entry.is_dir || entry.permissions & 0o170000 != 0o100000 {
                continue;
            }
            let windows = !plan.remote() && cfg!(windows);
            let executable = if windows {
                std::path::Path::new(&entry.name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("exe") || s.eq_ignore_ascii_case("com"))
            } else {
                entry.permissions & 0o111 != 0
            };
            if !executable {
                continue;
            }
            let (name, prefix) = if windows {
                (entry.name.to_lowercase(), plan.prefix.to_lowercase())
            } else {
                (entry.name.clone(), plan.prefix.clone())
            };
            if !name.starts_with(&prefix) {
                continue;
            }
            if plan.remote() && entry.name.contains(['/', '\\', '\0']) {
                result.skipped = true;
                continue;
            }
            let path = if windows && plan.dialect == 1 {
                entry.path.replace('\\', "/")
            } else {
                entry.path
            };
            let Some(command) = invocation(&path, plan.dialect, windows) else {
                result.skipped = true;
                continue;
            };
            let kind = if plan.remote() { 6 } else { 5 };
            // Keep only the best 50 while scanning; full path is the identity.
            candidates.insert(
                (name, command.clone()),
                CommandSuggestion {
                    kind,
                    label: entry.name,
                    detail: directory.clone(),
                    command,
                    ..Default::default()
                },
            );
            if candidates.len() > 50 {
                candidates.pop_last();
                result.truncated = true;
            }
        }
    }
    check(deadline, canceled)?;
    let mut bytes = 0;
    for (_, item) in candidates {
        bytes += item.command.len();
        if bytes > 65536 {
            result.truncated = true;
            break;
        }
        result.items.push(item);
    }
    Ok(result)
}
