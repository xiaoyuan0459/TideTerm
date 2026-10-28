//! Read-only login preview. Authentication still verifies the key on its own connection.

use crate::error::{Error, Result};
use crate::known_hosts::KnownHostStatus;
use std::net::Shutdown;

pub struct HostPreview {
    pub banner: String,
    pub fingerprint: String,
    pub status: KnownHostStatus,
}

pub async fn inspect_host(host: String, port: u16, timeout_secs: u64) -> Result<HostPreview> {
    let config = super::config::SshConfig {
        host: host.clone(),
        port,
        username: String::new(),
        auth: super::config::SshAuth::Agent,
        options: Default::default(),
        connect_timeout_secs: timeout_secs,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 2,
        term: String::new(),
        link_watchdog_secs: 0,
    };
    let (session, cancel) = super::connection::connect_session(&config).await?;
    let result = (|| {
        let (key, _) = session
            .host_key()
            .ok_or_else(|| Error::Ssh("server did not present a host key".into()))?;
        Ok(HostPreview {
            banner: session.banner().unwrap_or_default().to_owned(),
            fingerprint: super::handler::sha256_fingerprint(
                session.host_key_hash(ssh2::HashType::Sha256),
            ),
            status: crate::known_hosts::verify_status(&host, port, key)?,
        })
    })();
    let _ = cancel.shutdown(Shutdown::Both);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn silent_preview_times_out_and_releases_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut bytes = [0; 1024];
            loop {
                if stream.read(&mut bytes).unwrap() == 0 {
                    return;
                }
            }
        });
        let started = Instant::now();
        assert!(inspect_host("127.0.0.1".into(), port, 1).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        peer.join().unwrap();
    }
}
