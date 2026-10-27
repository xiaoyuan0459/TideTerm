//! Probe transports use the same host-key trust boundary as production SSH.
pub(super) fn require_known(
    session: &ssh2::Session,
    host: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiderm_core::known_hosts::{verify_status_path, KnownHostStatus};
    let path = std::env::var_os("TIDETERM_SSH_KNOWN_HOSTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(tiderm_core::known_hosts::default_known_hosts_path);
    let (key, _) = session
        .host_key()
        .ok_or("server did not present a host key")?;
    match verify_status_path(&path, host, port, key)? {
        KnownHostStatus::Known => Ok(()),
        KnownHostStatus::Unknown => {
            Err("unknown host key: verify and provision known_hosts before benchmarking".into())
        }
        KnownHostStatus::KeyChanged => Err("changed host key: benchmark refused".into()),
    }
}
