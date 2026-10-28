use super::*;

impl LibSshSftp {
    pub(super) async fn workflow_rename_new(&self, from: &str, to: &str) -> Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let from = from.to_owned();
        let to = to.to_owned();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.rename(
                Path::new(&from),
                Path::new(&to),
                Some(ssh2::RenameFlags::empty()),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|_| "SFTP move task failed")?
    }

    pub(super) async fn workflow_lstat_optional(
        &self,
        path: &str,
    ) -> Result<Option<SftpFileStat>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            let st = match sftp.lstat(Path::new(&path)) {
                Ok(st) => st,
                Err(error) if matches!(error.code(), ssh2::ErrorCode::SFTP(2 | 10)) => {
                    return Ok(None)
                }
                Err(error) => return Err(error.to_string()),
            };
            Ok(Some(SftpFileStat {
                accessed_unix: st.atime.map(|v| v as i64),
                size_bytes: st.size.unwrap_or(0),
                modified_unix: st.mtime.unwrap_or(0) as i64,
                permissions: st.perm.unwrap_or(0),
                is_dir: is_dir(st.perm),
                owner: None,
                group: None,
            }))
        })
        .await
        .map_err(|_| "SFTP lstat task failed")?
    }

    pub(super) async fn workflow_symlink(&self, target: &str, link: &str) -> Result<(), String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let target = target.to_owned();
        let link = link.to_owned();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.symlink(Path::new(&target), Path::new(&link))
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|_| "SFTP symlink task failed")?
    }

    pub(super) async fn workflow_write_new(
        &self,
        path: &str,
        mode: i32,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_owned();
        let file = tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            sftp.open_mode(
                Path::new(&path),
                ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE,
                mode,
                ssh2::OpenType::File,
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|_| "SFTP exclusive write task failed")??;
        Ok(Box::new(LibSftpWriteStream {
            inner: SftpStreamFile::new(file, self.session.clone(), self.blocking_mode_gate.clone()),
        }))
    }
}
