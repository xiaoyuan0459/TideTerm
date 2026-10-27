use super::*;
use std::{collections::BTreeMap, path::Path, process::Command};

fn hex(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn body(repo: &str, refs: &str) -> Vec<u8> {
    format!("3;fixture;b;0;{};{}", hex(repo), hex(refs)).into_bytes()
}

#[test]
fn decoder_rejects_malformed_budget_identity_and_noncanonical_references() {
    let good = body("/repo/.git", "refs/heads/main\nrefs/tags/v1");
    assert_eq!(decode(&good, "fixture", 1).unwrap().references.len(), 2);
    assert!(decode(&good, "other", 1).is_none());
    assert!(decode(&good, "fixture", 2).is_none());
    for refs in [
        "refs/remotes/a",
        "refs/heads/-x",
        "refs/heads/a..b",
        "refs/tags/a\r",
        "refs/heads/a\nrefs/heads/a",
        "refs/tags/a\nrefs/heads/a",
        "refs/heads/a.lock",
        "refs/heads/.x",
        "refs/tags/a//b",
    ] {
        assert!(
            decode(&body("/r", refs), "fixture", 1).is_none(),
            "accepted {refs:?}"
        );
    }
    for data in [
        body("", ""),
        body("/bad\nrepo", ""),
        body(&"r".repeat(2049), ""),
        body("/r", &"x".repeat(4097)),
        b"3;fixture;b;0;ff;".to_vec(),
        b"3;fixture;b;1;61;".to_vec(),
        b"3;fixture;b;0;2f72;;extra".to_vec(),
    ] {
        assert!(decode(&data, "fixture", 1).is_none());
    }
    let refs = (0..65)
        .map(|i| format!("refs/heads/r{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let report = decode(&body("/r", &refs), "fixture", 1).unwrap();
    assert!(report.truncated);
    assert_eq!(report.references.len(), 64);
    assert!(decode(&body("/r", &(refs + "\nrefs/heads/r65")), "fixture", 1).is_none());
}

fn git(repo: &Path, args: &[&str]) {
    let mut process = Command::new("git");
    isolate(&mut process);
    let output = process
        .current_dir(repo)
        .args(["-c", "core.hooksPath=", "-c", "init.templateDir="])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn isolate(process: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            process.env_remove(key);
        }
    }
    process
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1");
}
fn files(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(base: &Path, p: &Path, result: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                visit(base, &p, result);
            } else {
                result.insert(
                    p.strip_prefix(base).unwrap().into(),
                    std::fs::read(p).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
fn run(repo: &Path, dialect: u8) -> ShellReport {
    let shell = if dialect == 1 {
        if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "bash"
        }
    } else {
        "pwsh"
    };
    let mut p = Command::new(shell);
    isolate(&mut p);
    // An empty fixture may live below the checkout when TEMP is redirected.
    // Stop discovery at its parent so it cannot find the developer's repository.
    p.env("GIT_CEILING_DIRECTORIES", repo.parent().unwrap());
    if dialect == 1 {
        p.args(["--noprofile", "--norc", "-c"]);
    } else {
        p.args(["-NoProfile", "-NonInteractive", "-Command"]);
    }
    // A same-named function must not replace the explicitly chosen Git application.
    let shadow = if dialect == 1 {
        "git() { builtin printf SHADOW_RAN; }; "
    } else {
        "function git { throw 'SHADOW_RAN' }; "
    };
    let script = format!("{shadow}{}", command("fixture", dialect).unwrap());
    let output = p.current_dir(repo).arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "interpreter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut wire = crate::rust::shell_report_wire::Wire::default();
    let mut reports = vec![];
    for bytes in output.stdout.chunks(3) {
        let (visible, batch) = wire.feed(bytes);
        assert!(visible.is_empty());
        reports.extend(batch);
    }
    assert_eq!(reports.len(), 1, "missing fixed-query report");
    decode(&reports[0], "fixture", dialect).expect("real Git report rejected")
}

#[test]
fn real_git_queries_are_read_only_bounded_and_shell_literals_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    git(
        repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "commit",
            "-qm",
            "fixture",
            "--allow-empty",
        ],
    );
    for name in ["main", "目录", "quote'$(printf_INJECTED)"] {
        git(repo, &["branch", name]);
    }
    git(repo, &["tag", "main"]);
    for dialect in if cfg!(windows) { vec![1, 2] } else { vec![1] } {
        let before = files(repo);
        let mut report = run(repo, dialect);
        report.state = 3;
        assert_eq!(report.git_error, 0);
        assert!(!report.truncated);
        assert!(report.references.contains(&"refs/heads/目录".into()));
        assert!(report.references.contains(&"refs/tags/main".into()));
        assert!(report.repository.ends_with("/.git") || report.repository.ends_with("\\.git"));
        let switches = suggestions(&report, "git switch ");
        assert!(switches
            .iter()
            .all(|s| s.kind == 8 && s.command.starts_with("git switch -- '")));
        let shows = suggestions(&report, "git show main");
        assert_eq!(shows.len(), 2);
        assert_ne!(shows[0].command, shows[1].command);
        for query in [
            "git switch -x",
            "git checkout ",
            "git show 'x",
            "git show a;x",
            "git show $(x)",
            "git show x\ny",
            "git -C /x show ",
        ] {
            assert!(suggestions(&report, query).is_empty());
        }
        let value = suggestions(&report, "git show quote").pop().unwrap();
        let literal = value.command.strip_prefix("git show ").unwrap();
        let mut interpreter = Command::new(if dialect == 1 {
            if cfg!(windows) {
                "C:/Program Files/Git/bin/bash.exe"
            } else {
                "bash"
            }
        } else {
            "pwsh"
        });
        isolate(&mut interpreter);
        if dialect == 1 {
            interpreter
                .args(["--noprofile", "--norc", "-c"])
                .arg(format!("set -- {literal}; builtin printf '%s' \"$1\""));
        } else {
            interpreter
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(format!("$value={literal}; [Console]::Write($value)"));
        }
        let out = interpreter.current_dir(repo).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), value.reference);
        assert_eq!(
            files(repo),
            before,
            "metadata query or quoting changed repository files"
        );
    }
    for i in 0..66 {
        git(repo, &["branch", &format!("r{i:02}")]);
    }
    for dialect in if cfg!(windows) { vec![1, 2] } else { vec![1] } {
        let report = run(repo, dialect);
        assert!(report.truncated);
        assert_eq!(report.references.len(), 64);
    }
    for i in 0..30 {
        git(repo, &["branch", &format!("a{i:02}{}", "x".repeat(180))]);
    }
    for dialect in if cfg!(windows) { vec![1, 2] } else { vec![1] } {
        let report = run(repo, dialect);
        assert_ne!(report.git_error, 0);
        assert!(report.references.is_empty());
        let empty = tempfile::tempdir().unwrap();
        assert_ne!(run(empty.path(), dialect).git_error, 0);
    }
}
