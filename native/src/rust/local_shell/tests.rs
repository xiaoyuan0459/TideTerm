use super::*;
use crate::rust::{session_form_config, store::session_store};
use std::{sync::Arc, time::Duration};
use tiderm_core::{
    config::SessionStore,
    protocol::{pty_impl::PtyInstance, Protocol},
};

/// Saved form options must reach a real shell unchanged: executable override,
/// quoted argv, Unicode working directory, TERM, output and exit status.
#[tokio::test]
async fn saved_local_options_launch_real_shells() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("cwd space 中文");
    std::fs::create_dir(&cwd).expect("cwd");
    let canonical_cwd = std::fs::canonicalize(&cwd).expect("canonical cwd");
    let legacy = format!(
        r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
        std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
    );
    let cases = [
        (
            "pwsh",
            "",
            r#"-NoLogo -NoProfile -NonInteractive -Command "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); [Console]::WriteLine('TT_' + 'PWSH_' + $PSVersionTable.PSVersion.Major); [Console]::WriteLine($env:TERM); [Console]::WriteLine((Split-Path -Leaf $PWD)); exit 23""#,
            "TT_PWSH_7",
        ),
        ("pwsh", "", "", "TT_PWSH_7"),
        (
            "cmd",
            legacy.as_str(),
            r#"-NoLogo -NoProfile -NonInteractive -Command "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); [Console]::WriteLine('TT_' + 'WINPS_' + $PSVersionTable.PSVersion.Major); [Console]::WriteLine($env:TERM); [Console]::WriteLine((Split-Path -Leaf $PWD)); exit 23""#,
            "TT_WINPS_5",
        ),
        (
            "git-bash",
            "",
            r#"--noprofile --norc -c "printf 'TT_%s\n' GIT_BASH; printf '%s\n' \"$TERM\" \"${PWD##*/}\"; exit 23""#,
            "TT_GIT_BASH",
        ),
    ];
    for (shell, path, args, marker) in cases {
        let mut config = super::super::store::session_config::SessionConfig::blank(
            super::super::store::session_config::SessionKind::Local,
        );
        config.id = shell.into();
        config.local = Some(LocalConfig {
            shell: shell.into(),
            shell_path: path.into(),
            args: args.into(),
            working_directory: cwd.to_string_lossy().into_owned(),
        });
        let form = session_form_config::cfg_to_pod(&config);
        let config = session_form_config::pod_to_cfg(&form);
        let store_path = dir.path().join("sessions.json");
        let mut store = SessionStore::new(store_path.clone());
        store.upsert(session_store::to_core(&config));
        store.save().expect("save");
        let loaded = SessionStore::load_at(store_path).expect("reload");
        let config = session_store::from_core(loaded.find(shell).expect("saved shell"));
        let form = session_form_config::cfg_to_pod(&config);
        assert_eq!(
            (&*form.local_shell, &*form.local_path, &*form.local_args),
            (shell, path, args)
        );
        let local = config.local.expect("local options");
        let launch = launch_options(&local).expect("argv");
        let program = &launch.program;
        let mut argv = launch.args.clone();
        if args.is_empty() {
            // Reproduce an English Windows console independently of the host
            // locale. The app's real startup script must restore UTF-8 before
            // emitting its prompt and the later Unicode cwd witness.
            argv.last_mut()
                .expect("interactive startup script")
                .insert_str(
                    0,
                    "[Console]::OutputEncoding=[Text.Encoding]::GetEncoding(437); ",
                );
        }
        eprintln!(
            "shell={shell} program={program:?} argv={argv:?} cwd={:?}",
            local.working_directory
        );
        let pty = Arc::new(
            PtyInstance::spawn_with_options(
                80,
                24,
                program,
                &argv,
                Some(&local.working_directory),
                "vt100",
            )
            .expect("configured shell launches"),
        );
        let resize = pty.resize(120, 40).await;
        let read_pty = pty.clone();
        let interactive = args.is_empty();
        let required_cwd = canonical_cwd.clone();
        let mut pump = tokio::spawn(async move {
            let mut output = Vec::new();
            let mut cursor_replies = 0;
            let mut command_sent = false;
            loop {
                let data = read_pty.read().await?;
                if data.is_empty() {
                    break;
                }
                eprintln!("shell output: {:?}", String::from_utf8_lossy(&data));
                output.extend(data);
                let text = String::from_utf8_lossy(&output);
                // Headless PTY: answer cursor-position queries as a terminal
                // would. The script supplies the real success marker.
                let requested = text.matches("\x1b[6n").count();
                for _ in cursor_replies..requested {
                    read_pty.write(b"\x1b[1;1R").await?;
                }
                cursor_replies = requested;
                if interactive
                    && !command_sent
                    && text.contains("tideterm-history")
                    && text.contains("PS ")
                {
                    read_pty.write(b"[Console]::WriteLine('TT_' + 'PWSH_' + $PSVersionTable.PSVersion.Major); [Console]::WriteLine($env:TERM); [Console]::WriteLine((Split-Path -Leaf $PWD)); exit 23\r").await?;
                    command_sent = true;
                }
                if text.contains(marker)
                    && text.contains("vt100")
                    && text.contains("cwd space 中文")
                    && (!interactive || cwd_event_matches(&text, &required_cwd))
                {
                    break;
                }
            }
            Ok::<_, Error>(output)
        });
        let output = tokio::time::timeout(Duration::from_secs(8), &mut pump).await;
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while pty.is_alive() && std::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let exit_code = pty.exit_code();
        let disconnected = pty.disconnect().await;
        pump.abort();
        drop(pump);
        drop(pty);
        resize.expect("resize");
        disconnected.expect("disconnect");
        let output = output
            .expect("shell output deadline")
            .expect("pump")
            .expect("read");
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains(marker) && text.contains("vt100") && text.contains("cwd space 中文"),
            "shell={shell}, output={text:?}"
        );
        assert_eq!(exit_code, Some(23), "shell={shell}");
        if interactive {
            assert!(text.contains("tideterm-history"), "prompt integration");
            assert!(
                cwd_event_matches(&text, &canonical_cwd),
                "Unicode current-directory event"
            );
        }
    }
}

fn cwd_event_matches(output: &str, expected: &std::path::Path) -> bool {
    // Windows TEMP may contain an 8.3 alias (RUNNER~1), while PowerShell emits
    // the long provider path. Compare the real directory, preserving Unicode.
    output.split("\x1b]9;9;").skip(1).any(|event| {
        event.split_once('\x07').is_some_and(|(path, _)| {
            std::fs::canonicalize(path).is_ok_and(|observed| observed == expected)
        })
    })
}
