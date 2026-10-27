//! R-25 regression at the real authentication boundary, using generated keys.
#![cfg(windows)]

use base64::Engine;
use std::{
    io::{BufRead, BufReader, Read},
    net::TcpStream,
    process::{Command, Stdio},
};
use tiderm_core::protocol::ssh::{auth::authenticate, SshAuth, SshConfig};

struct Peer(std::process::Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn check(info: &serde_json::Value, auth: SshAuth, username: &str) -> Result<(), String> {
    let port = info["port"].as_u64().unwrap() as u16;
    let mut session = ssh2::Session::new().map_err(|e| e.to_string())?;
    session.set_timeout(5000);
    session.set_tcp_stream(TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?);
    session.handshake().map_err(|e| e.to_string())?;
    // Pin the independently generated server key before sending credentials.
    let expected = base64::engine::general_purpose::STANDARD
        .decode(info["host_key"].as_str().unwrap())
        .unwrap();
    assert_eq!(session.host_key().unwrap().0, expected);
    let config = SshConfig {
        host: "127.0.0.1".into(),
        port,
        username: username.into(),
        auth,
        connect_timeout_secs: 5,
        reconnect_enabled: false,
        max_reconnect_attempts: 0,
        reconnect_interval_secs: 0,
        link_watchdog_secs: 0,
        options: Default::default(),
        term: "xterm".into(),
    };
    authenticate(&session, &config).map_err(|e| e.to_string())?;
    assert!(session.authenticated());
    let mut channel = session.channel_session().map_err(|e| e.to_string())?;
    channel
        .exec("fixture-round-trip")
        .map_err(|e| e.to_string())?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| e.to_string())?;
    assert_eq!(output, "KEY_AUTH_OK\n");
    Ok(())
}

#[test]
#[ignore = "requires Windows Python + Paramiko/cryptography; isolated loopback and agent"]
fn windows_ssh_private_key_container_encryption_and_agent_matrix() {
    if let Ok(metadata) = std::env::var("TIDETERM_KEY_FIXTURE") {
        let info = serde_json::from_str(&metadata).unwrap();
        check(&info, SshAuth::Agent, "fixture")
            .expect("isolated agent authentication and round trip");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ssh_key_peer.py");
    let mut peer = Peer(
        Command::new("python")
            .arg(script)
            .arg(dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Python fixture must be installed"),
    );
    let mut line = String::new();
    BufReader::new(peer.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).expect("fixture ready metadata");
    let mut failures = Vec::new();
    let mut observe = |name: &str, auth, username, accepted| {
        let result = check(&info, auth, username);
        let passed = if accepted {
            result.is_ok()
        } else {
            result.as_ref().is_err_and(|error| error.contains("auth:"))
        };
        println!(
            "{name}: {} ({})",
            if passed { "PASS" } else { "FAIL" },
            if result.is_ok() {
                "accepted"
            } else {
                "rejected"
            }
        );
        if !passed {
            failures.push(format!("{name}: {result:?}"));
        }
    };
    for case in info["cases"].as_array().unwrap() {
        let encrypted = case["encrypted"].as_bool().unwrap();
        let auth = SshAuth::PublicKey {
            private_key_path: case["path"].as_str().unwrap().into(),
            passphrase: encrypted.then(|| "fixture-passphrase".into()),
        };
        observe(
            case["name"].as_str().unwrap(),
            auth.clone(),
            "fixture",
            true,
        );
        if encrypted {
            let mut wrong = auth;
            if let SshAuth::PublicKey { passphrase, .. } = &mut wrong {
                *passphrase = Some("incorrect-fixture-passphrase".into());
            }
            observe(
                &format!("{}-wrong-passphrase", case["name"].as_str().unwrap()),
                wrong,
                "fixture",
                false,
            );
        }
    }
    observe(
        "unauthorized-key",
        SshAuth::PublicKey {
            private_key_path: info["untrusted"].as_str().unwrap().into(),
            passphrase: None,
        },
        "fixture",
        false,
    );
    observe(
        "password-control",
        SshAuth::Password {
            password: "fixture-password".into(),
        },
        "fixture",
        true,
    );
    observe(
        "wrong-user-control",
        SshAuth::Password {
            password: "fixture-password".into(),
        },
        "other",
        false,
    );
    // Start a child so the C runtime sees SSH_AUTH_SOCK at process startup.
    for (algorithm, pipe) in info["agents"].as_object().unwrap() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows_ssh_private_key_container_encryption_and_agent_matrix",
                "--ignored",
            ])
            .env("TIDETERM_KEY_FIXTURE", info.to_string())
            .env("SSH_AUTH_SOCK", pipe.as_str().unwrap())
            .output()
            .unwrap();
        println!(
            "{algorithm}-agent: {}",
            if result.status.success() {
                "PASS"
            } else {
                "FAIL"
            }
        );
        if !result.status.success() {
            failures.push(format!(
                "{algorithm}-agent: {}",
                String::from_utf8_lossy(&result.stdout)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} matrix failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
