use super::*;
use std::path::Path;

fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn channel(name: &str, dialect: u8) -> Channel {
    Channel {
        pending: Some(Pending {
            token: "fixture".into(),
            dialect,
            query: Query::Lookup(name.into()),
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
fn bash_path(path: &Path) -> String {
    let path = path.to_str().unwrap().replace('\\', "/");
    if cfg!(windows) {
        format!("/{}{}", path[..1].to_lowercase(), &path[2..])
    } else {
        path
    }
}
fn run(dialect: u8, setup: &str, name: &str) -> ShellReport {
    let probe = shell_resolution::command("fixture", dialect, name).unwrap();
    let script = if dialect == 1 {
        format!("{setup}; {probe}")
    } else {
        format!("{setup}; $ttOuterPreference=$PSModuleAutoLoadingPreference; {probe}; if ($PSModuleAutoLoadingPreference -ne $ttOuterPreference) {{ throw 'lookup changed caller preference' }}")
    };
    let mut cmd = std::process::Command::new(if dialect == 1 {
        if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "bash"
        }
    } else {
        "pwsh"
    });
    if dialect == 1 {
        cmd.args(["--noprofile", "--norc", "-c"]);
    } else {
        cmd.args(["-NoProfile", "-NonInteractive", "-Command"]);
    }
    let output = cmd.arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "isolated lookup interpreter failed"
    );
    let mut report = channel(name, dialect);
    let mut visible = Vec::new();
    for chunk in output.stdout.chunks(3) {
        visible.extend(report.output(chunk));
    }
    assert!(
        visible.is_empty(),
        "lookup ran a target or exposed extra output"
    );
    assert_eq!(
        report.view(0).state,
        3,
        "lookup did not produce an admitted report"
    );
    report.view(0)
}

// This owner guards mode/name admission. Shared v1 framing and lifecycle remain
// in shell_report_tests; interpreter tests below own actual lookup semantics.
#[test]
fn resolution_reports_bind_mode_name_budget_directory_and_non_command_kinds() {
    for name in [
        "", ".", "../x", "-x", "a b", "a*", "a;echo", "$(id)", "中文",
    ] {
        assert!(shell_resolution::command("fixture", 1, name).is_none());
    }
    let mut c = channel("Tide_Query", 1);
    for body in [
        "1;fixture;b;2f70617468",
        "2;other;b;function;Tide_Query;",
        "2;fixture;b;function;Other;",
        "2;fixture;w;function;Tide_Query;",
        "2;fixture;b;function;Tide_Query;41",
        "2;fixture;b;file;Tide_Query;",
    ] {
        c.output(format!("\x1b]777;tideterm-env;{body}\x07").as_bytes());
        assert_eq!(c.view(0).state, 2);
    }
    for location in ["x".repeat(4097), "bad\npath".into()] {
        c.output(
            format!(
                "\x1b]777;tideterm-env;2;fixture;b;file;Tide_Query;{}\x07",
                hex(&location)
            )
            .as_bytes(),
        );
        assert_eq!(c.view(0).state, 2);
    }
    c.output(b"\x1b]777;tideterm-env;2;fixture;b;function;Tide_Query;\x07");
    assert!(c.view(0).can_fill);
    c.validate(0, 0, 1);
    assert_eq!(c.view(0).state, 6);
    for kind in ["missing", "keyword"] {
        let mut c = channel("if", 1);
        c.output(format!("\x1b]777;tideterm-env;2;fixture;b;{kind};if;\x07").as_bytes());
        assert_eq!(c.view(0).state, 3);
        assert!(!c.view(0).can_fill);
    }
}

#[test]
fn bash_resolution_preserves_alias_function_hash_and_path_priority_without_running_targets() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    for directory in [&first, &second] {
        std::fs::create_dir(directory).unwrap();
        let file = directory.join("TIDE_QUERY");
        std::fs::write(&file, "#!/bin/sh\nprintf TARGET_EXECUTED\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let marker = temp.path().join("executed");
    let base = format!(
        "PATH='{}:{}'; TIDE_QUERY() {{ builtin printf TARGET > '{}'; }}",
        bash_path(&first),
        bash_path(&second),
        bash_path(&marker)
    );
    let report = run(1, &base, "TIDE_QUERY");
    assert_eq!(report.kind, 2);
    assert!(report.location.is_empty());
    let report = run(
        1,
        &format!("{base}; shopt -s expand_aliases; alias TIDE_QUERY=TIDE_TARGET"),
        "TIDE_QUERY",
    );
    assert_eq!(report.kind, 1);
    let base = format!("PATH='{}:{}'", bash_path(&first), bash_path(&second));
    let report = run(1, &base, "TIDE_QUERY");
    assert_eq!(report.kind, 5);
    assert_eq!(report.location, format!("{}/TIDE_QUERY", bash_path(&first)));
    let report = run(
        1,
        &format!(
            "{base}; builtin hash -p '{}/TIDE_QUERY' TIDE_QUERY",
            bash_path(&second)
        ),
        "TIDE_QUERY",
    );
    assert_eq!(
        report.location,
        format!("{}/TIDE_QUERY", bash_path(&second))
    );
    for (name, kind) in [("printf", 3), ("if", 4), ("TIDE_MISSING", 0)] {
        let report = run(1, &base, name);
        assert_eq!(report.kind, kind);
    }
    assert!(!marker.exists(), "lookup executed the function");
}

#[test]
#[cfg(windows)]
fn powershell_lookup_keeps_loaded_precedence_and_does_not_import_or_run_modules() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    let app = first.join("TIDE_QUERY.exe");
    assert!(std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/program_probe.rs"
        ))
        .arg("-o")
        .arg(&app)
        .output()
        .unwrap()
        .status
        .success());
    std::fs::copy(&app, second.join("TIDE_QUERY.exe")).unwrap();
    let marker = temp.path().join("executed");
    let path = format!("$env:PATH='{};{}'", first.display(), second.display());
    let setup = format!(
        "{path}; function Tide_Query {{ [IO.File]::WriteAllText('{}','ran') }}",
        marker.display()
    );
    assert_eq!(run(2, &setup, "Tide_Query").kind, 2);
    assert_eq!(
        run(
            2,
            &format!("{setup}; Set-Alias Tide_Query Tide_Target"),
            "Tide_Query"
        )
        .kind,
        1
    );
    assert_eq!(
        run(
            2,
            &format!(
                "{path}; filter Tide_Query {{ [IO.File]::WriteAllText('{}','ran') }}",
                marker.display()
            ),
            "Tide_Query"
        )
        .kind,
        8
    );
    let app_report = run(2, &path, "TIDE_QUERY");
    assert_eq!(app_report.kind, 5);
    assert_eq!(app_report.location, app.to_str().unwrap());
    let cmdlet = run(2, &path, "Get-Command");
    assert_eq!(cmdlet.kind, 7);
    assert_eq!(cmdlet.location, "Microsoft.PowerShell.Core");
    let script = first.join("TIDE_SCRIPT.ps1");
    std::fs::write(
        &script,
        format!("[IO.File]::WriteAllText('{}','ran')", marker.display()),
    )
    .unwrap();
    let report = run(2, &path, "TIDE_SCRIPT.ps1");
    assert_eq!(report.kind, 6);
    assert_eq!(report.location, script.to_str().unwrap());
    let modules = temp.path().join("modules");
    let module = modules.join("TideLookupOnly");
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(module.join("TideLookupOnly.psm1"), format!("[IO.File]::WriteAllText('{}','imported'); function Get-TidePrivate {{ 'private' }}; Export-ModuleMember -Function Get-TidePrivate", marker.display())).unwrap();
    std::fs::write(module.join("TideLookupOnly.psd1"), "@{ RootModule='TideLookupOnly.psm1'; ModuleVersion='1.0'; FunctionsToExport=@('Get-TidePrivate') }").unwrap();
    let setup = format!(
        "{path}; $env:PSModulePath='{}'; $PSModuleAutoLoadingPreference='All'",
        modules.display()
    );
    assert_eq!(run(2, &setup, "Get-TidePrivate").kind, 0);
    assert!(!marker.exists(), "lookup imported a module or ran a target");
}
