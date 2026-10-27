use super::super::{
    store::session_config::{SessionConfig, SessionKind},
    term,
};
use super::*;

fn pending(dialect: u8, token: &str) -> Channel {
    Channel {
        pending: Some(Pending {
            token: token.into(),
            dialect,
            query: Query::Path,
            directory_revision: None,
            generation: 0,
            input_revision: 0,
            deadline: Instant::now() + Duration::from_secs(10),
            sent: true,
        }),
        state: 2,
        ..Default::default()
    }
}
fn envelope(token: &str, platform: &str, path: &str) -> Vec<u8> {
    let hex: String = path.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("\x1b]777;tideterm-env;1;{token};{platform};{hex}\x07").into_bytes()
}

// Wire is the primary owner of budget, framing and report privacy. No parser
// assertions are replayed in Qt; Qt owns user actions and retirement instead.
#[test]
fn bounded_reports_preserve_other_output_and_require_a_live_single_request() {
    let mut channel = pending(1, "expected");
    let mut input = b"before\x1b]9;9;/cwd\x07".to_vec();
    input.extend(envelope("other", "b", "/wrong"));
    input.extend(envelope("expected", "w", "/wrong-platform"));
    input.extend(envelope("expected", "b", "/目录:/a b::relative"));
    input.extend(envelope("expected", "b", "/replay"));
    input.extend(b"after");
    let mut visible = Vec::new();
    for b in input {
        visible.extend(channel.output(&[b]));
    }
    assert_eq!(visible, b"before\x1b]9;9;/cwd\x07after");
    assert_eq!(channel.view(1).path, "/目录:/a b::relative");
    channel.clear();
    assert!(channel
        .output(&envelope("expected", "b", "/late"))
        .is_empty());
    assert_eq!(channel.view(1).state, 0);
    let mut malformed = b"\x1b]777;tideterm-env;".to_vec();
    malformed.extend(vec![b'x'; 20000]);
    malformed.extend(b"\x1b[31mRECOVERED");
    assert_eq!(channel.output(&malformed), b"\x1b[31mRECOVERED");
    for path in ["x".repeat(8193), ":".repeat(128), "bad\npath".into()] {
        let mut c = pending(1, "expected");
        c.output(&envelope("expected", "b", &path));
        assert_eq!(c.view(1).state, 2);
    }
    let mut c = pending(1, "expected");
    let st = envelope("expected", "b", "/ST")
        .into_iter()
        .take_while(|b| *b != 7)
        .chain([27, b'\\'])
        .collect::<Vec<_>>();
    assert!(c.output(&st).is_empty());
    assert_eq!(c.view(1).path, "/ST");
    c.snapshot.as_mut().unwrap().at = Instant::now() - Duration::from_secs(61);
    c.validate(0, 0, 0);
    assert_eq!(c.view(1).state, 7);
    let mut c = pending(1, "expected");
    c.output(&envelope("expected", "b", "/generation"));
    c.validate(1, 0, 0);
    assert_eq!(c.view(1).state, 6);
    let mut c = pending(1, "expired");
    c.pending.as_mut().unwrap().deadline = Instant::now();
    c.validate(0, 0, 0);
    assert_eq!(c.view(1).state, 4);
}

// Real interpreters generate their own payload from a deliberately changed
// interactive PATH. The oracle is synthetic, never the developer's environment.
#[test]
fn real_shell_commands_report_only_the_session_path_without_executing_its_entries() {
    let cases = if cfg!(windows) {
        vec![(1, "C:/Program Files/Git/bin/bash.exe"), (2, "pwsh")]
    } else {
        vec![(1, "bash")]
    };
    for (dialect, program) in cases {
        let path = if dialect == 1 {
            "/only/目录:/space name::relative"
        } else {
            "C:\\only\\目录;C:\\space name;;relative"
        };
        let probe = wire::command("fixture", dialect).unwrap();
        let script = if dialect == 1 {
            format!("PATH='{path}'; {probe}")
        } else {
            format!("$env:PATH='{path}'; {probe}")
        };
        let mut process = std::process::Command::new(program);
        if dialect == 1 {
            process.args(["--noprofile", "--norc", "-c"]);
        } else {
            process.args(["-NoProfile", "-NonInteractive", "-Command"]);
        }
        let output = process.arg(script).output().unwrap();
        assert!(output.status.success(), "isolated interpreter failed");
        let mut channel = pending(dialect, "fixture");
        for chunk in output.stdout.chunks(3) {
            channel.output(chunk);
        }
        assert_eq!(channel.view(0).state, 3, "interpreter report rejected");
        assert_eq!(channel.view(0).path, path);
    }
}

#[test]
fn explicit_report_roundtrip_and_lifecycle_use_the_real_session_writer() {
    let _network = super::super::term_net_tests::net_serial();
    let _serial = super::super::term_tests::input_lock_guard();
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    let local = cfg.local.as_mut().unwrap();
    local.shell = if cfg!(windows) { "pwsh" } else { "bash" }.into();
    let tab = term::open_tab_with_config(&cfg);
    let wait = |f: &dyn Fn() -> bool| {
        let until = Instant::now() + Duration::from_secs(12);
        while Instant::now() < until {
            term::term_poll(tab);
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    };
    assert!(wait(&|| {
        super::super::command_completion::completion_context(tab).ready
    }));
    // Use an isolated synthetic value and a marker produced by the interpreter.
    let (dialect, value, assignment) = if cfg!(windows) {
        (
            2,
            "C:\\TIDE_REPORT_ONLY",
            "$env:PATH='C:\\TIDE_REPORT_ONLY'; [Console]::WriteLine(('TIDE_'+'READY'))\r",
        )
    } else {
        (
            1,
            "/TIDE_REPORT_ONLY",
            "PATH=/TIDE_REPORT_ONLY; builtin printf 'TIDE_%s\\n' READY\r",
        )
    };
    super::super::term_input::term_write(tab, assignment.as_bytes());
    assert!(wait(
        &|| super::super::term_diag::term_screen_text(tab).contains("TIDE_READY")
    ));
    let preview = shell_report_prepare(tab, dialect);
    term::with_tab(tab, |st| st.log.set_on(true));
    let other = term::open_tab_with_config(&cfg);
    let other_preview = shell_report_prepare(other, dialect);
    assert_eq!(shell_report_send(other, &preview.token), 6);
    assert_eq!(shell_report_poll(other).token, other_preview.token);
    term::term_close_tab(other);
    assert_eq!(preview.state, 1);
    assert_eq!(shell_report_poll(tab).state, 1);
    assert_eq!(shell_report_send(tab, "wrong"), 6);
    assert_eq!(shell_report_send(tab, &preview.token), 2);
    assert_eq!(shell_report_send(tab, &preview.token), 6);
    assert!(wait(&|| shell_report_poll(tab).state == 3));
    assert_eq!(shell_report_poll(tab).path, value);
    let screen = super::super::term_diag::term_screen_text(tab);
    let encoded: String = value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    assert!(
        !screen.contains(&encoded),
        "OSC payload leaked into the screen"
    );
    assert!(!term::with_tab(tab, |st| st.log.recent(200)).contains(&encoded));
    let define = if cfg!(windows) {
        "function global:Tide_Query { [Console]::WriteLine(('LOOKUP_'+'EXECUTED')) }; [Console]::WriteLine(('LOOKUP_'+'READY'))\r"
    } else {
        "Tide_Query() { builtin printf 'LOOKUP_%s\\n' EXECUTED; }; builtin printf 'LOOKUP_%s\\n' READY\r"
    };
    super::super::term_input::term_write(tab, define.as_bytes());
    assert!(wait(
        &|| super::super::term_diag::term_screen_text(tab).contains("LOOKUP_READY")
    ));
    let lookup = shell_resolution_prepare(tab, dialect, "Tide_Query");
    assert_eq!(shell_report_send(tab, &lookup.token), 2);
    assert!(wait(&|| shell_report_poll(tab).state == 3));
    assert_eq!(shell_report_poll(tab).kind, 2);
    assert!(shell_resolution_draft(tab, "wrong").is_empty());
    assert_eq!(shell_resolution_draft(tab, &lookup.token), "Tide_Query");
    assert!(shell_resolution_draft(tab, &lookup.token).is_empty());
    assert!(!super::super::term_diag::term_screen_text(tab).contains("LOOKUP_EXECUTED"));
    super::super::term_input::term_write(tab, b"Tide_Query\r");
    assert!(wait(
        &|| super::super::term_diag::term_screen_text(tab).contains("LOOKUP_EXECUTED")
    ));
    let lookup = shell_resolution_prepare(tab, dialect, "Tide_Query");
    assert_eq!(shell_report_send(tab, &lookup.token), 2);
    assert!(wait(&|| shell_report_poll(tab).state == 3));
    let before = super::super::command_completion::completion_context(tab).directory_revision;
    let changed = if cfg!(windows) {
        "[Console]::Write(([char]27+']9;9;C:\\TIDE_LOOKUP_CWD'+[char]7))\r"
    } else {
        "builtin printf '\\033]9;9;/TIDE_LOOKUP_CWD\\007'\r"
    };
    super::super::term_input::term_write(tab, changed.as_bytes());
    assert!(wait(
        &|| super::super::command_completion::completion_context(tab).directory_revision > before
    ));
    assert!(shell_resolution_draft(tab, &lookup.token).is_empty());
    let preview = shell_report_prepare(tab, dialect);
    super::super::term_reading::term_set_reading(tab, true);
    super::super::term_reading::term_set_reading(tab, false);
    assert_eq!(shell_report_send(tab, &preview.token), 6);
    let preview = shell_report_prepare(tab, dialect);
    super::super::term_ops::term_set_locked(true);
    super::super::term_ops::term_set_locked(false);
    assert_eq!(shell_report_send(tab, &preview.token), 6);
    let preview = shell_report_prepare(tab, dialect);
    term::with_tab(tab, |st| st.pane.feed("\x1b[?1049h"));
    assert_eq!(shell_report_send(tab, &preview.token), 6);
    term::with_tab(tab, |st| st.pane.feed("\x1b[?1049l"));
    term::term_close_tab(tab);
    assert_eq!(shell_report_poll(tab).state, 6);
}
