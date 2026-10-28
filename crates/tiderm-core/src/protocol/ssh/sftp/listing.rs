use super::*;
use crate::sftp::{DirectoryQuery, DirectoryReadError as Failure, DirectorySnapshot};

impl LibSshSftp {
    pub(super) async fn list_bounded(
        &self,
        path: &str,
        query: DirectoryQuery,
    ) -> Result<DirectorySnapshot, Failure> {
        query.check()?;
        if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            return Err(Failure::InvalidRequest);
        }
        let (sftp, session, gate) = (
            self.sftp.clone(),
            self.session.clone(),
            self.blocking_mode_gate.clone(),
        );
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            query.check()?;
            let file = {
                let _mode = SftpCallGuard::new(&session, &gate).map_err(Failure::Io)?;
                query.check()?;
                sftp.opendir(Path::new(&path)).map_err(failure)?
            };
            let directory = SftpStreamFile::new(file, session, gate);
            let result = read(&directory, &path, &query);
            // Directory close is libssh2 IO too; reuse the stream's gated owner.
            drop(directory);
            query.check()?;
            result
        })
        .await
        .map_err(|e| Failure::Io(format!("directory task: {e}")))?
    }
}

fn failure(error: ssh2::Error) -> Failure {
    if error.code() == ssh2::ErrorCode::Session(libssh2_sys::LIBSSH2_ERROR_TIMEOUT) {
        Failure::TimedOut
    } else {
        Failure::Io(error.to_string())
    }
}

fn read(
    directory: &SftpStreamFile,
    path: &str,
    query: &DirectoryQuery,
) -> Result<DirectorySnapshot, Failure> {
    let mut result = DirectorySnapshot::default();
    let mut bytes = 0;
    let mut examined = 0;
    loop {
        query.check()?;
        if examined == query.max_entries {
            result.truncated = true;
            break;
        }
        // Retain ssh2's EOF/error code while the shared mode guard covers one call.
        let next = directory
            .with_file(|file| Ok(file.readdir()))
            .map_err(|e| Failure::Io(e.to_string()))?;
        query.check()?;
        let (name, stat) = match next {
            Ok(entry) => entry,
            Err(error)
                if error.code() == ssh2::ErrorCode::Session(libssh2_sys::LIBSSH2_ERROR_FILE) =>
            {
                break
            }
            Err(error)
                if error.code() == ssh2::ErrorCode::Session(libssh2_sys::LIBSSH2_ERROR_EAGAIN) =>
            {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error)
                if error.code() == ssh2::ErrorCode::Session(libssh2_sys::LIBSSH2_ERROR_TIMEOUT) =>
            {
                return Err(Failure::TimedOut)
            }
            Err(error) => return Err(Failure::Io(error.to_string())),
        };
        examined += 1;
        let Some(name) = name.to_str() else {
            continue;
        };
        if matches!(name, "." | ".." | "") || name.contains(['/', '\0']) {
            continue;
        }
        // Remote path spelling is independent of the client's platform.
        let full_path = format!("{}/{}", path.trim_end_matches('/'), name);
        bytes += name.len() + full_path.len();
        if bytes > query.max_name_bytes {
            result.truncated = true;
            break;
        }
        result.entries.push(SftpFileEntry {
            name: name.to_owned(),
            path: full_path,
            is_dir: is_dir(stat.perm),
            size_bytes: stat.size.unwrap_or(0),
            modified_unix: stat.mtime.unwrap_or(0) as i64,
            permissions: stat.perm.unwrap_or(0),
            owner: None,
            group: None,
        });
    }
    Ok(result)
}
