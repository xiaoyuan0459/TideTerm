//! Real libssh2/SSH callbacks -> local X TCP and Windows SSH Agent named pipe.
use super::{SshAuth, SshConfig, SshInstance};
use crate::{protocol::Protocol, session_options::SshOptions};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Child(std::process::Child);
struct RejectUnknown;
#[async_trait::async_trait]
impl crate::auth::HostKeyInquirer for RejectUnknown {
    async fn confirm(
        &self,
        _: &str,
        _: &str,
        _: u16,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> Result<bool, String> {
        Ok(false)
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "requires local Python + Paramiko; no external SSH server"]
fn real_ssh_x11_cookie_replacement_and_agent_forwarding_close_with_session() {
    let _guard = crate::test_config_guard();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ssh_forward_peer.py");
        let mut child = Child(
            Command::new("python")
                .arg(script)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let mut line = String::new();
        BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let info: serde_json::Value = serde_json::from_str(&line).unwrap();
        let port = info["port"].as_u64().unwrap() as u16;
        let known = dir.path().join("known_hosts");
        std::fs::write(
            &known,
            format!(
                "[127.0.0.1]:{port} ssh-rsa {}\n",
                info["key"].as_str().unwrap()
            ),
        )
        .unwrap();
        let previous_known = std::env::var_os("TEST_SSH_KNOWN_HOSTS");
        let previous_agent = std::env::var_os("SSH_AUTH_SOCK");
        // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
        unsafe { std::env::set_var("TEST_SSH_KNOWN_HOSTS", &known) };
        let pipe_path = format!(r"\\.\pipe\tideterm-agent-fixture-{}", uuid::Uuid::new_v4());
        // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
        unsafe { std::env::set_var("SSH_AUTH_SOCK", &pipe_path) };
        let pipe = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_path)
            .unwrap();
        let agent = tokio::spawn(async move {
            // First connection is the availability probe; second carries Agent frames.
            let mut pipe = pipe;
            for index in 0..2 {
                pipe.connect().await.unwrap();
                if index == 0 {
                    let mut byte = [0];
                    assert_eq!(pipe.read(&mut byte).await.unwrap(), 0);
                } else {
                    let mut frame = [0; 5];
                    pipe.read_exact(&mut frame).await.unwrap();
                    assert_eq!(frame, [0, 0, 0, 1, 11]);
                    pipe.write_all(&[0, 0, 0, 5, 12, 0, 0, 0, 0]).await.unwrap();
                    let mut eof = [0];
                    assert_eq!(pipe.read(&mut eof).await.unwrap(), 0);
                }
                pipe.disconnect().unwrap();
                if index == 0 {
                    pipe = tokio::net::windows::named_pipe::ServerOptions::new()
                        .create(&pipe_path)
                        .unwrap();
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let xport = listener.local_addr().unwrap().port();
        assert!(xport >= 6000);
        let authority = dir.path().join("Xauthority");
        let number = (xport - 6000).to_string();
        let mut auth = 0u16.to_be_bytes().to_vec();
        for field in [
            &[127, 0, 0, 1][..],
            number.as_bytes(),
            b"MIT-MAGIC-COOKIE-1",
            &[7u8; 16],
        ] {
            auth.extend((field.len() as u16).to_be_bytes());
            auth.extend(field);
        }
        std::fs::write(&authority, auth).unwrap();
        let xserver = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut setup = [0; 48];
            stream.read_exact(&mut setup).await.unwrap();
            assert_eq!(&setup[32..], &[7; 16]);
            let mut request = [0; 9];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"X_REQUEST");
            stream.write_all(b"X_REPLY").await.unwrap();
            let mut eof = [0];
            assert_eq!(stream.read(&mut eof).await.unwrap(), 0);
        });
        let config = SshConfig {
            host: "127.0.0.1".into(),
            port,
            username: "fixture".into(),
            auth: SshAuth::Password {
                password: "fixture".into(),
            },
            connect_timeout_secs: 10,
            reconnect_enabled: false,
            max_reconnect_attempts: 0,
            reconnect_interval_secs: 1,
            term: "xterm".into(),
            link_watchdog_secs: 0,
            options: SshOptions {
                kex: "diffie-hellman-group14-sha256".into(),
                agent_forwarding: true,
                x11_enabled: true,
                x11_display: format!("127.0.0.1:{number}"),
                x11_auth_file: authority.to_string_lossy().into(),
                ..Default::default()
            },
        };
        let proto = SshInstance::connect(
            config,
            80,
            24,
            "fixture".into(),
            &RejectUnknown,
            Arc::new(crate::auth::NoopKbBackend),
        )
        .await
        .unwrap();
        // Shared SFTP initialization may temporarily enable blocking mode.
        // A genuine idle read timeout must not kill the terminal or its forwards.
        let session = proto.session.lock().await.clone();
        let previous_timeout = session.timeout();
        session.set_timeout(50);
        session.set_blocking(true);
        tokio::time::sleep(Duration::from_millis(180)).await;
        assert!(
            proto.is_alive(),
            "idle blocking read terminated the SSH session"
        );
        session.set_blocking(false);
        session.set_timeout(previous_timeout);
        proto.write(b"START").await.unwrap();
        let output = tokio::time::timeout(Duration::from_secs(15), async {
            let mut bytes = Vec::new();
            while !bytes.windows(10).any(|b| b == b"FORWARD_OK") {
                let chunk = proto.read().await.unwrap();
                assert!(!chunk.is_empty());
                bytes.extend(chunk);
            }
            bytes
        })
        .await
        .unwrap();
        assert!(output.windows(10).any(|b| b == b"FORWARD_OK"));
        proto.disconnect().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            agent.await.unwrap();
            xserver.await.unwrap();
        })
        .await
        .unwrap();
        match previous_known {
            // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
            Some(v) => unsafe { std::env::set_var("TEST_SSH_KNOWN_HOSTS", v) },
            // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
            None => unsafe { std::env::remove_var("TEST_SSH_KNOWN_HOSTS") },
        }
        match previous_agent {
            // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
            Some(v) => unsafe { std::env::set_var("SSH_AUTH_SOCK", v) },
            // SAFETY: this module is Windows-only; Windows environment mutation is thread-safe.
            None => unsafe { std::env::remove_var("SSH_AUTH_SOCK") },
        }
    });
}
