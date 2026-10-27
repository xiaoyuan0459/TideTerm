//! Transfer metadata and safe local names, independent of the desktop shell.
use crate::session_options::SessionOptions;
use crate::sftp::{SftpFileStat, SftpTransport};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn preserve_upload_times(
    transport: &dyn SftpTransport,
    local: &Path,
    remote: &str,
    options: &SessionOptions,
) -> Result<(), String> {
    if !options.sftp_preserve_modified && !options.sftp_preserve_accessed {
        return Ok(());
    }
    let metadata = tokio::fs::metadata(local)
        .await
        .map_err(|e| e.to_string())?;
    let seconds = |time: std::io::Result<SystemTime>| {
        time.ok()
            .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
            .map(|v| v.as_secs())
    };
    let modified = options
        .sftp_preserve_modified
        .then(|| seconds(metadata.modified()))
        .flatten();
    let accessed = options
        .sftp_preserve_accessed
        .then(|| seconds(metadata.accessed()))
        .flatten();
    transport.set_times(remote, accessed, modified).await
}

pub async fn preserve_download_times(
    local: &Path,
    metadata: &SftpFileStat,
    options: &SessionOptions,
) -> Result<(), String> {
    let mut times = std::fs::FileTimes::new();
    if options.sftp_preserve_modified && metadata.modified_unix >= 0 {
        times = times.set_modified(
            UNIX_EPOCH + std::time::Duration::from_secs(metadata.modified_unix as u64),
        );
    }
    if options.sftp_preserve_accessed
        && let Some(value) = metadata.accessed_unix.filter(|v| *v >= 0)
    {
        times = times.set_accessed(UNIX_EPOCH + std::time::Duration::from_secs(value as u64));
    }
    let local = local.to_owned();
    tokio::task::spawn_blocking(move || {
        std::fs::OpenOptions::new()
            .write(true)
            .open(local)?
            .set_times(times)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Remote names must remain a single local path component. Windows also has
/// reserved device names and characters that are valid on Unix servers.
pub fn local_file_name(name: &str, repair: bool) -> Result<String, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err("invalid remote file name".into());
    }
    if !cfg!(windows) {
        return Ok(name.to_owned());
    }
    let invalid = |c: char| c < ' ' || "<>:\"|?*\\".contains(c);
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    let needs_repair = reserved || name.ends_with(['.', ' ']) || name.chars().any(invalid);
    if needs_repair && !repair {
        return Err("remote file name is invalid on Windows; enable name repair".into());
    }
    let mut result: String = name
        .chars()
        .map(|c| if invalid(c) { '_' } else { c })
        .collect();
    result = result.trim_end_matches(['.', ' ']).to_owned();
    if reserved {
        result.insert(0, '_');
    }
    if result.is_empty() {
        result.push('_');
    }
    Ok(result)
}
