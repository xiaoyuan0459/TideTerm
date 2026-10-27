//! The directory boundary owns quoting; real shells validate literal arguments.
use super::*;
use crate::rust::{environment_draft, environment_paths};

#[test]
fn quoted_directory_candidates_reach_real_shells_as_one_literal_argument() {
    let root = tempfile::tempdir().unwrap();
    let name = "quote space'; echo INJECTED; '.txt";
    std::fs::write(root.path().join(name), b"LITERAL_PATH_ONCE\n").unwrap();
    std::fs::create_dir(root.path().join("quote folder's")).unwrap();
    std::fs::write(
        root.path().join("quote folder's/child's.txt"),
        b"LITERAL_PATH_ONCE\n",
    )
    .unwrap();
    let cwd = root.path().to_str().unwrap();
    let shells = if cfg!(windows) {
        vec![
            (2, "Get-Content -LiteralPath ", "pwsh"),
            (1, "cat ", "C:/Program Files/Git/bin/bash.exe"),
        ]
    } else {
        vec![(1, "cat ", "sh")]
    };
    for (dialect, head, shell) in shells {
        for draft in [
            format!("{head}./quote"),
            format!("{head}'./quote"),
            format!("{head}'./quote'"),
            if dialect == 2 {
                format!("{head}'./quote folder''s/child")
            } else {
                format!("{head}'./quote folder'\\''s/child")
            },
        ] {
            let plan = environment_paths::plan(cwd, &draft, dialect).unwrap();
            let result =
                environment_paths::scan(&plan, Instant::now() + Duration::from_secs(3), || true);
            assert_eq!(result.state, 2);
            let command = result
                .items
                .iter()
                .find(|i| i.command.ends_with(".txt'"))
                .unwrap()
                .command
                .clone();
            let mut process = std::process::Command::new(shell);
            if dialect == 2 {
                process.args(["-NoProfile", "-NonInteractive", "-Command"]);
            } else {
                process.arg("-c");
            }
            let output = process
                .arg(&command)
                .current_dir(root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{command}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
                "LITERAL_PATH_ONCE\n"
            );
            // Generated spelling remains usable for the next query; no shell
            // expansion, wildcard interpretation or command-position & is added.
            assert!(environment_paths::plan(cwd, &command, dialect).is_some());
        }
    }
    for dialect in 0..=2 {
        for draft in [
            "cat ./x;echo",
            "cat ./x | echo",
            "cat ./$(id)",
            "cat \"./x\"",
            "cat './x' ./y",
        ] {
            assert!(
                environment_paths::plan(cwd, draft, dialect).is_none(),
                "{dialect}: {draft}"
            );
        }
    }
    assert!(environment_draft::parse("cat ./", 3, false).is_none());
    assert!(environment_draft::parse("./quote", 1, false)
        .unwrap()
        .command("./quote space")
        .is_none());
    assert!(environment_draft::parse("cat ./", 2, true)
        .unwrap()
        .command("./a[1].txt")
        .is_none());
    assert!(environment_draft::parse("cat ./", 2, true)
        .unwrap()
        .command("./a’ ; echo BAD; ’")
        .is_none());
}
