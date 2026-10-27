use super::*;
use std::process::{Command, Stdio};

/// The app hook must preserve user callbacks and status, report executed UTF-8
/// commands once, and remove its private startup file when ownership ends.
#[test]
fn prompt_hook_preserves_profile_and_reports_executed_commands() {
    let profiles = [
        "PROMPT_COMMAND=user_prompt",
        "PROMPT_COMMAND=(user_prompt user_second_prompt)",
    ];
    for profile in profiles {
        let fixture = tempfile::tempdir().expect("fixture HOME");
        let rc = format!(
            "unset HISTFILE\nPS1='fixture> '\nuser_prompt() {{ local status=$?; printf 'USER_CALLBACK\\n'; return \"$status\"; }}\nuser_second_prompt() {{ local status=$?; printf 'USER_ARRAY\\n'; return \"$status\"; }}\n{profile}\n"
        );
        std::fs::write(fixture.path().join(".bashrc"), rc).expect("fixture rc");
        let init = startup_file(true).expect("app startup file");
        let init_path = init.path().to_owned();
        let mut child = Command::new("bash")
            .args(["--noprofile", "--rcfile"])
            .arg(&init_path)
            .arg("-i")
            .env("HOME", fixture.path())
            .env_remove("HISTCONTROL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("interactive Bash");
        let command = "printf '%s\\n' 'ECHO 中文'";
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(
                format!("false\nprintf 'STATUS=%s\\n' \"$?\"\n{command}\nread -rs fixture_secret\nNOT_A_COMMAND_PASSWORD\n\nexit\n").as_bytes(),
            )
            .expect("commands");
        let output = child.wait_with_output().expect("Bash exits");
        assert!(output.status.success(), "{:?}", output.status);
        let output = String::from_utf8(output.stdout).expect("UTF-8 output");
        assert!(
            output.contains("USER_CALLBACK"),
            "user callback was replaced"
        );
        if profile.contains('(') {
            assert!(
                output.contains("USER_ARRAY"),
                "user callback array was replaced"
            );
        }
        assert!(
            output.contains("STATUS=1"),
            "prompt hook masked command status: {output:?}"
        );
        let commands: Vec<_> = output
            .split("\x1b]777;tideterm-command;")
            .skip(1)
            .filter_map(|event| event.split_once('\x07'))
            .map(|(encoded, _)| {
                percent_encoding::percent_decode_str(encoded)
                    .decode_utf8()
                    .expect("encoded UTF-8")
                    .into_owned()
            })
            .collect();
        assert_eq!(
            commands.iter().filter(|entry| *entry == command).count(),
            1,
            "repeated prompt must not duplicate commands: {commands:?}"
        );
        assert!(
            commands.iter().any(|entry| entry == "false"),
            "executed failure was omitted"
        );
        assert!(
            commands
                .iter()
                .all(|entry| !entry.contains("NOT_A_COMMAND_PASSWORD")),
            "non-command input was recorded"
        );
        drop(init);
        assert!(
            !init_path.exists(),
            "private startup file survived its owner"
        );
    }
}
