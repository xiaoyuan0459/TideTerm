use super::*;

fn payload(rows: &str) -> Vec<u8> {
    let hex: String = rows.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("4;fixture;b;0;fixture;{hex}").into_bytes()
}
fn row(id: &str, name: &str) -> String {
    format!(
        "{}|{}",
        serde_json::to_string(id).unwrap(),
        serde_json::to_string(name).unwrap()
    )
}
#[test]
fn docker_identity_budget_and_literal_context_are_admitted_before_drafts() {
    let id = "a".repeat(64);
    let first = row(&id, "ui_alpha");
    let mut report = decode(&payload(&first), "fixture", 1, "fixture").unwrap();
    report.state = 3;
    assert_eq!(
        suggestions(&report, "docker logs ui_")[0].command,
        format!("docker --context 'fixture' container logs '{id}'")
    );
    for (token, dialect, context) in [
        ("other", 1, "fixture"),
        ("fixture", 2, "fixture"),
        ("fixture", 1, "other"),
    ] {
        assert!(decode(&payload(&first), token, dialect, context).is_none());
    }
    for context in ["", "-x", "a b", "$(x)", "a;run", "a'b", "../ctx"] {
        assert!(!valid_context(context));
    }
    for query in [
        "docker exec ",
        "docker logs x;run",
        "docker logs 'x",
        "docker logs x\nrun",
        "docker --context other logs ",
        "docker logs $(x)",
    ] {
        assert!(suggestions(&report, query).is_empty());
    }
    for rows in [
        format!("{first}\n{}", row(&id, "ui_other")),
        format!("{first}\n{}", row(&"b".repeat(64), "ui_alpha")),
        row(&"a".repeat(12), "short"),
        row(&id, "unsafe name"),
        row(&id, "bad\rname"),
        row(&id, "bad\tname"),
        row(&id, &"x".repeat(129)),
        "x".repeat(6145),
    ] {
        assert!(decode(&payload(&rows), "fixture", 1, "fixture").is_none());
    }
    let rows = (0..33)
        .map(|i| row(&format!("{i:064x}"), &format!("c{i}")))
        .collect::<Vec<_>>()
        .join("\n");
    let report = decode(&payload(&rows), "fixture", 1, "fixture").unwrap();
    assert!(report.truncated);
    assert_eq!(report.containers.len(), 32);
    assert!(decode(
        &payload(&(rows + "\n" + &row(&format!("{:064x}", 33), "c33"))),
        "fixture",
        1,
        "fixture"
    )
    .is_none());
}

#[test]
#[ignore = "requires isolated Docker CLI/context and loopback docker_engine_peer.py; never use a user daemon"]
fn real_docker_cli_reports_bounded_metadata_in_bash_and_powershell() {
    use std::{path::PathBuf, process::Command};
    let exe = PathBuf::from(std::env::var("TIDETERM_TEST_DOCKER_EXE").expect("isolated CLI"));
    let root = PathBuf::from(std::env::var("TIDETERM_TEST_DOCKER_ROOT").expect("isolated fixture"));
    assert!(root
        .canonicalize()
        .unwrap()
        .starts_with(std::env::temp_dir().canonicalize().unwrap()));
    let config = root.join("config");
    let stats = || -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(root.join("stats.json")).unwrap()).unwrap()
    };
    for mode in [0, 1, 2, 3, 4] {
        std::fs::write(
            root.join("state.json"),
            match mode {
                0 => r#"{"count":2}"#,
                1 => r#"{"count":33}"#,
                2 => r#"{"count":33,"long":true}"#,
                3 => r#"{"fail":true}"#,
                _ => r#"{"count":2,"nul":true}"#,
            },
        )
        .unwrap();
        for dialect in if cfg!(windows) { vec![1, 2] } else { vec![1] } {
            let before = stats();
            let mut process = Command::new(if dialect == 1 {
                if cfg!(windows) {
                    "C:/Program Files/Git/bin/bash.exe"
                } else {
                    "bash"
                }
            } else {
                "pwsh"
            });
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("DOCKER_") {
                    process.env_remove(name);
                }
            }
            process
                .env("DOCKER_CONFIG", &config)
                .env("DOCKER_HOST", "tcp://127.0.0.1:9")
                .env("DOCKER_CONTEXT", "wrong-context");
            let path = std::env::join_paths(
                std::iter::once(exe.parent().unwrap().to_path_buf())
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap();
            process.env("PATH", path);
            if dialect == 1 {
                process.args(["--noprofile", "--norc", "-c"]);
            } else {
                process.args(["-NoProfile", "-NonInteractive", "-Command"]);
            }
            let shadow = if dialect == 1 {
                "docker() { builtin printf SHADOW_RAN; }; "
            } else {
                "function docker { throw 'SHADOW_RAN' }; "
            };
            let output = process
                .arg(format!(
                    "{shadow}{}",
                    command("fixture", dialect, "fixture").unwrap()
                ))
                .output()
                .unwrap();
            assert!(output.status.success(), "interpreter failed");
            let mut wire = crate::rust::shell_report_wire::Wire::default();
            let (visible, reports) = wire.feed(&output.stdout);
            assert!(visible.is_empty());
            assert_eq!(reports.len(), 1);
            let decoded = decode(&reports[0], "fixture", dialect, "fixture");
            if mode == 4 {
                assert!(
                    decoded
                        .as_ref()
                        .is_none_or(|r| r.docker_error != 0 && r.containers.is_empty()),
                    "control byte admitted as a valid truncated name"
                );
            } else {
                let report = decoded.expect("real CLI report rejected");
                if mode < 2 {
                    assert_eq!(report.docker_error, 0);
                    assert_eq!(report.containers.len(), if mode == 0 { 2 } else { 32 });
                    assert_eq!(report.truncated, mode == 1);
                    assert_eq!(report.containers[0].id, format!("{:064x}", 1));
                } else {
                    assert_ne!(report.docker_error, 0);
                    assert!(report.containers.is_empty());
                }
            }
            let after = stats();
            assert_eq!(
                after["lists"].as_u64().unwrap(),
                before["lists"].as_u64().unwrap() + 1
            );
            assert_eq!(after["bad_query"], 0);
            assert_eq!(after["forbidden"], 0);
        }
    }
    std::fs::write(root.join("state.json"), r#"{"count":2}"#).unwrap();
}
