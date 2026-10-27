use super::*;

impl LibSshSftp {
    pub(super) async fn update_times(
        &self,
        path: &str,
        accessed: Option<u64>,
        modified: Option<u64>,
    ) -> Result<(), String> {
        if accessed.is_none() && modified.is_none() {
            return Ok(());
        }
        let sftp = self.sftp.clone();
        let session = self.session.clone();
        let gate = self.blocking_mode_gate.clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let _mode = SftpCallGuard::new(&session, &gate)?;
            // SFTP v3 sends atime/mtime together. Preserve the unspecified value.
            let current = sftp.stat(Path::new(&path)).map_err(|e| e.to_string())?;
            sftp.setstat(
                Path::new(&path),
                ssh2::FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: None,
                    atime: accessed.or(current.atime),
                    mtime: modified.or(current.mtime),
                },
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("set times task failed: {e}"))?
    }
}
