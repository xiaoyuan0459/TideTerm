use super::*;

fn channel(dialect: u8, prefix: &str) -> Channel {
    Channel {
        pending: Some(Pending {
            token: "names-fixture".into(),
            dialect,
            query: Query::Names(prefix.into()),
            generation: 0,
            input_revision: 0,
            directory_revision: Some(0),
            deadline: Instant::now() + Duration::from_secs(10),
            sent: true,
        }),
        state: 2,
        ..Default::default()
    }
}
fn bash_path(path: &std::path::Path) -> String {
    let p = path.to_str().unwrap().replace('\\', "/");
    if cfg!(windows) {
        format!("/{}{}", p[..1].to_lowercase(), &p[2..])
    } else {
        p
    }
}
fn collect(dialect: u8, setup: &str, prefix: &str) -> ShellReport {
    let probe = super::super::shell_names::command("names-fixture", dialect, prefix).unwrap();
    let script = if dialect == 1 {
        format!("{setup}; {probe}")
    } else {
        format!("{setup}; $outerPreference=$PSModuleAutoLoadingPreference; {probe}; if ($PSModuleAutoLoadingPreference -ne $outerPreference) {{ throw 'changed caller preference' }}")
    };
    let mut p = std::process::Command::new(if dialect == 1 {
        if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "bash"
        }
    } else {
        "pwsh"
    });
    p.args(if dialect == 1 {
        vec!["--noprofile", "--norc", "-c"]
    } else {
        vec!["-NoProfile", "-NonInteractive", "-Command"]
    });
    let out = p.arg(script).output().unwrap();
    assert!(
        out.status.success(),
        "isolated catalog failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut c = channel(dialect, prefix);
    for chunk in out.stdout.chunks(3) {
        assert!(
            c.output(chunk).is_empty(),
            "catalog exposed visible output or ran a target"
        );
    }
    let r = c.view(0);
    assert_eq!(r.state, 3);
    assert_eq!(r.name_error, 0);
    assert_eq!(
        r.system,
        if cfg!(windows) {
            1
        } else if cfg!(target_os = "macos") {
            3
        } else {
            2
        },
        "the real Shell must report its own OS"
    );
    r
}

// Interpreter execution owns discovery/no-execution semantics. Existing framing
// tests own generic OSC privacy; Qt owns normal entry and draft acceptance.
#[test]
fn names_discovery_preserves_shell_scope_and_never_runs_targets_or_imports_modules() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("executed");
    let file = root.path().join("sysExternal");
    std::fs::write(
        &file,
        format!("#!/bin/sh\nprintf ran > '{}'\n", bash_path(&marker)),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let r = collect(
        1,
        &format!(
            "PATH='{}'; shopt -s expand_aliases; sysFunction() {{ builtin printf ran > '{}'; }}; alias sysAlias=sysFunction",
            bash_path(root.path()),
            bash_path(&marker)
        ),
        "sys",
    );
    for (name, kind) in [("sysAlias", 1), ("sysFunction", 2), ("sysExternal", 5)] {
        assert!(
            r.names.iter().any(|n| n.name == name && n.kind == kind),
            "missing {name}"
        );
    }
    let builtins = collect(1, "PATH=/nonexistent", "printf");
    assert!(builtins
        .names
        .iter()
        .any(|n| n.name == "printf" && n.kind == 3));
    #[cfg(windows)]
    {
        std::fs::write(
            root.path().join("sysExternal.ps1"),
            format!("[IO.File]::WriteAllText('{}','ran')", marker.display()),
        )
        .unwrap();
        let module = root.path().join("modules/TideNames");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("TideNames.psm1"),format!("[IO.File]::WriteAllText('{}','imported'); function sysGhost {{ 'private' }}; Export-ModuleMember -Function sysGhost", marker.display())).unwrap();
        std::fs::write(module.join("TideNames.psd1"),"@{ RootModule='TideNames.psm1'; ModuleVersion='1.0'; FunctionsToExport=@('sysGhost') }").unwrap();
        let setup = format!("$env:PATH='{}'; $env:PSModulePath='{}'; $PSModuleAutoLoadingPreference='All'; function sysFunction {{ [IO.File]::WriteAllText('{}','ran') }}; Set-Alias sysAlias sysFunction",root.path().display(),root.path().join("modules").display(),marker.display());
        let r = collect(2, &setup, "sys");
        for (name, kind) in [("sysAlias", 1), ("sysFunction", 2), ("sysExternal.ps1", 6)] {
            assert!(
                r.names.iter().any(|n| n.name == name && n.kind == kind),
                "missing {name}"
            );
        }
        assert!(!r.names.iter().any(|n| n.name == "sysGhost"));
    }
    assert!(
        !marker.exists(),
        "discovery invoked a target or imported a module"
    );
}

#[test]
fn names_catalog_is_bounded_ranked_and_survives_edits_but_retires_with_source() {
    let setup =
        "PATH=/nonexistent; for ((i=0;i<300;i++)); do builtin eval \"sys$i() { :; }\"; done";
    let r = collect(1, setup, "sys");
    assert!(r.truncated);
    assert_eq!(r.names.len(), 256);
    let mut c = channel(1, "");
    let frame = b"\x1b]777;tideterm-env;N;names-fixture;b;0;0;2:737973507265666978,2:6d79737973,2:736c6f775973,4:7379734b6579776f7264,9:737973436f6e666967,\x07";
    c.output(frame);
    c.clear_request();
    c.validate(0, 99, 0);
    let r = c.catalog.poll(0, 1);
    let rows = super::super::shell_names::suggestions(&r, "sys");
    assert_eq!(
        rows.iter().map(|s| s.command.as_str()).collect::<Vec<_>>(),
        ["sysPrefix", "mysys", "slowYs"]
    );
    assert!(rows.iter().all(|s| s.kind == 12));
    c.validate(0, 99, 1);
    assert_eq!(c.catalog.poll(0, 1).state, 6);
    let mut c = channel(1, "");
    c.output(frame);
    c.snapshot.as_mut().unwrap().at = Instant::now() - Duration::from_secs(61);
    c.validate(0, 0, 0);
    assert_eq!(c.catalog.poll(0, 1).state, 7);
    for prefix in ["x;run", "$(id)", "中文", &"a".repeat(65)] {
        assert!(super::super::shell_names::command("names-fixture", 1, prefix).is_none());
    }
    let mut c = channel(1, "sys");
    for invalid in [
        "N;wrong;b;0;0;2:737973",
        "N;names-fixture;w;0;0;2:737973",
        "N;names-fixture;b;1;0;2:737973",
    ] {
        c.output(format!("\x1b]777;tideterm-env;{invalid}\x07").as_bytes());
        assert_eq!(c.view(0).state, 2);
    }
}
