//! Known-hosts verification for SSH connections (de-shelled from `src-tauri/known_hosts.rs`).
//!
//! Uses `ssh2::KnownHosts` (libssh2) with the **raw** server host-key bytes obtained
//! from `Session::host_key` after the handshake. Unknown hosts require frontend
//! confirmation; key mismatches are rejected as potential MITM attacks.

use crate::error::{Error, Result};
use ssh2::{CheckResult, KnownHostFileKind, KnownHostKeyFormat};
use std::path::{Path, PathBuf};

/// Outcome of a known_hosts verification check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownHostStatus {
    /// Key matches a known entry.
    Known,
    /// Host is unknown (first connection).
    Unknown,
    /// Key has changed since last connection (possible MITM).
    KeyChanged,
}

/// Default known_hosts file path (OpenSSH convention: `~/.ssh/known_hosts`).
pub fn default_known_hosts_path() -> PathBuf {
    if let Some(directory) = crate::PROFILE_DIRECTORY.get() {
        return directory.join("known_hosts");
    }
    // The ignored SSH environment tests accept their controlled sshd's key.
    // Keep that write inside an explicitly supplied fixture, never in ~/.ssh.
    #[cfg(test)]
    if let Some(path) = std::env::var_os("TEST_SSH_KNOWN_HOSTS") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".ssh")
        .join("known_hosts")
}

/// Shared verification over a specific known_hosts file.
fn verify_in(path: &Path, host: &str, port: u16, key: &[u8]) -> Result<KnownHostStatus> {
    let session = ssh2::Session::new().map_err(|e| Error::Ssh(format!("known_hosts init: {e}")))?;
    let mut known_hosts = session
        .known_hosts()
        .map_err(|e| Error::Ssh(format!("known_hosts init: {e}")))?;
    let _ = known_hosts.read_file(path, KnownHostFileKind::OpenSSH);
    match known_hosts.check_port(host, port, key) {
        CheckResult::Match => Ok(KnownHostStatus::Known),
        CheckResult::NotFound => Ok(KnownHostStatus::Unknown),
        CheckResult::Mismatch => Ok(KnownHostStatus::KeyChanged),
        CheckResult::Failure => Err(Error::Ssh(format!(
            "known_hosts check failed for {host}:{port}"
        ))),
    }
}

/// Verify a server's raw host key against the default known_hosts file.
pub fn verify_status(host: &str, port: u16, key: &[u8]) -> Result<KnownHostStatus> {
    verify_in(&default_known_hosts_path(), host, port, key)
}

/// Verify a server's raw host key against a specific known_hosts file.
pub fn verify_status_path(
    path: &Path,
    host: &str,
    port: u16,
    key: &[u8],
) -> Result<KnownHostStatus> {
    verify_in(path, host, port, key)
}

/// Shared "learn" over a specific known_hosts file.
fn learn_in(
    path: &Path,
    host: &str,
    port: u16,
    key: &[u8],
    key_format: KnownHostKeyFormat,
) -> Result<()> {
    let session = ssh2::Session::new().map_err(|e| Error::Ssh(format!("known_hosts init: {e}")))?;
    let mut known_hosts = session
        .known_hosts()
        .map_err(|e| Error::Ssh(format!("known_hosts init: {e}")))?;
    let _ = known_hosts.read_file(path, KnownHostFileKind::OpenSSH);
    let host_port = format!("[{host}]:{port}");
    known_hosts
        .add(&host_port, key, "", key_format)
        .map_err(|e| Error::Ssh(format!("learn known_hosts: {e}")))?;
    known_hosts
        .write_file(path, KnownHostFileKind::OpenSSH)
        .map_err(|e| Error::Ssh(format!("learn known_hosts write: {e}")))?;
    Ok(())
}

/// Learn (add) a server's raw host key to the default known_hosts file.
pub fn learn(host: &str, port: u16, key: &[u8], key_format: KnownHostKeyFormat) -> Result<()> {
    learn_in(&default_known_hosts_path(), host, port, key, key_format)
}

/// Learn (add) a server's raw host key to a specific known_hosts file.
pub fn learn_path(
    path: &Path,
    host: &str,
    port: u16,
    key: &[u8],
    key_format: KnownHostKeyFormat,
) -> Result<()> {
    learn_in(path, host, port, key, key_format)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_key(seed: u8) -> Vec<u8> {
        let mut key = b"ssh-ed25519\0".to_vec();
        key.extend(std::iter::repeat_n(seed, 32));
        key
    }

    #[test]
    fn unknown_host_returns_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        let status = verify_status_path(&path, "test-host.invalid", 22, &fake_key(1))
            .expect("verify empty file");
        assert_eq!(status, KnownHostStatus::Unknown);
    }

    #[test]
    fn learn_and_verify_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        let host = "test-learn-host.invalid";
        let port = 22;
        let key = fake_key(7);

        learn_path(&path, host, port, &key, KnownHostKeyFormat::Ed25519).expect("learn");
        let status = verify_status_path(&path, host, port, &key).expect("verify after learn");
        assert_eq!(status, KnownHostStatus::Known);
    }

    #[test]
    fn mismatched_key_returns_keychanged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        let host = "test-mismatch-host.invalid";
        let port = 22;

        learn_path(&path, host, port, &fake_key(1), KnownHostKeyFormat::Ed25519).expect("learn");
        let status = verify_status_path(&path, host, port, &fake_key(2)).expect("verify");
        assert_eq!(status, KnownHostStatus::KeyChanged);
    }
}
