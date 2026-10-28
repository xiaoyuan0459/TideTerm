//! Real loopback Telnet peers own the inserted-byte and no-execution contract.
use super::{term, terminal_completion as completion};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn peer() -> (u32, TcpStream) {
    use super::store::session_config::{SessionConfig, SessionKind};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = SessionConfig::blank(SessionKind::Telnet);
    let options = config.telnet.as_mut().unwrap();
    options.host = "127.0.0.1".into();
    options.port = listener.local_addr().unwrap().port();
    let id = term::open_tab_with_config(&config);
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            term::with_tab(id, |st| st.host.connected())
        }
    ));
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    // Consume the transport's initial negotiation before observing user bytes.
    let mut negotiation = [0; 512];
    while matches!(stream.read(&mut negotiation), Ok(n) if n != 0) {}
    stream.write_all(b"lab@host:$ ec").unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == "ec"
        }
    ));
    term::with_tab(id, |st| st.context.history.record("echo __TIDE_ALPHA__"));
    (id, stream)
}

fn silent(stream: &mut TcpStream) {
    let mut byte = [0];
    match stream.read(&mut byte) {
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) => {}
        result => panic!("unexpected bytes or closure: {result:?}, {byte:?}"),
    }
}

mod prompt_redraw;

#[test]
fn terminal_fill_is_one_shot_without_enter_and_ignores_sync_recipients() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (id, mut stream) = peer();
    let (other, mut other_stream) = peer();
    let batch = completion::terminal_completion_query(id);
    let at = batch
        .batch
        .items
        .iter()
        .position(|x| x.command == "echo __TIDE_ALPHA__")
        .unwrap() as u32;
    let sync = super::sync_channels_tests::fixture(&[id, other]);
    assert!(completion::terminal_completion_accept(&batch, at));
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = [0; 17];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"ho __TIDE_ALPHA__");
    stream.write_all(&bytes).unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == "echo __TIDE_ALPHA__"
        }
    ));
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    assert!(!completion::terminal_completion_accept(&batch, at));
    silent(&mut stream);
    silent(&mut other_stream);
    stream.write_all(b"\r\x1b[2Klab@host:$ ALPHA").unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == "ALPHA"
        }
    ));
    let replacement = completion::terminal_completion_query(id);
    let at = replacement
        .batch
        .items
        .iter()
        .position(|x| x.command == "echo __TIDE_ALPHA__")
        .unwrap() as u32;
    assert!(completion::terminal_completion_accept(&replacement, at));
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let expected = b"\x7f\x7f\x7f\x7f\x7fecho __TIDE_ALPHA__";
    let mut bytes = vec![0; expected.len()];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, expected);
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    silent(&mut stream);
    silent(&mut other_stream);
    drop(sync);
    term::term_close_tab(id);
    term::term_close_tab(other);
}

#[test]
fn terminal_fill_rejects_input_changes_before_echo_and_new_output() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (id, mut stream) = peer();
    let old = completion::terminal_completion_query(id);
    term::term_key(id, 0, 0, "x");
    assert!(
        !completion::terminal_completion_query(id)
            .batch
            .context
            .ready,
        "new snapshots must wait for the edited draft echo"
    );
    assert!(!completion::terminal_completion_accept(&old, 0));
    let mut typed = [0];
    stream.read_exact(&mut typed).unwrap();
    assert_eq!(&typed, b"x");
    stream.write_all(b"x").unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == "ecx"
        }
    ));
    let old = completion::terminal_completion_query(id);
    stream.write_all(b"\r\nbackground output").unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            !completion::terminal_completion_query(id)
                .batch
                .context
                .ready
        }
    ));
    assert!(!completion::terminal_completion_accept(&old, 0));
    silent(&mut stream);
    term::term_close_tab(id);
    assert!(
        !completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    assert!(!completion::terminal_completion_accept(&old, 0));
}

#[test]
fn terminal_fill_excludes_reading_and_alternate_screens() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (id, mut stream) = peer();
    assert!(super::term_reading::term_set_reading(id, true));
    assert!(
        !completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    assert!(super::term_reading::term_set_reading(id, false));
    assert!(
        completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    // Arbitrary prompt decoration works through the Shell's explicit input anchor.
    stream
        .write_all(b"\r\x1b[2K\x1b]133;A\x07decorated> \x1b]133;B\x07ec")
        .unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == "ec"
        }
    ));
    stream
        .write_all(b"\x1b]133;C\x07\r\nlab@host:$ ec")
        .unwrap();
    assert!(
        super::term_net_tests::wait_until(Duration::from_secs(5), || {
            term::term_poll(id);
            !completion::terminal_completion_query(id)
                .batch
                .context
                .ready
        }),
        "program output that resembles a prompt cannot reopen suggestions"
    );
    let history = super::command_history::history_commands(id);
    term::with_tab(id, |st| super::command_history::submitted(st, b"secret\r"));
    assert_eq!(super::command_history::history_commands(id), history);
    silent(&mut stream);
    stream.write_all(b"\x1b[?1049h\x1b[Hlab@host:$ ec").unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            !completion::terminal_completion_query(id)
                .batch
                .context
                .ready
        }
    ));
    silent(&mut stream);
    term::term_close_tab(id);
}

#[test]
fn terminal_draft_preserves_typed_trailing_spaces_before_filling() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (id, mut stream) = peer();
    let stats = super::term_diag::term_stats(id);
    term::term_resize_px(id, 12.0 * stats.cell_w as f32, 3.0 * stats.cell_h as f32);
    let mut negotiation = [0; 512];
    while matches!(stream.read(&mut negotiation), Ok(n) if n != 0) {}
    let before = completion::terminal_completion_query(id).output_revision;
    stream
        .write_all("\x1b[2J\x1b[Hlab@host:路径$ echo ".as_bytes())
        .unwrap();
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(id);
            completion::terminal_completion_query(id).output_revision != before
        }
    ));
    let snapshot = completion::terminal_completion_query(id);
    assert_eq!(snapshot.query, "echo ");
    let at = snapshot
        .batch
        .items
        .iter()
        .position(|x| x.command == "echo __TIDE_ALPHA__")
        .unwrap() as u32;
    assert!(completion::terminal_completion_accept(&snapshot, at));
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut actual = [0; 14];
    stream.read_exact(&mut actual).unwrap();
    assert_eq!(&actual, b"__TIDE_ALPHA__");
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    silent(&mut stream);
    term::term_close_tab(id);
}

// Allow cold PowerShell startup and asynchronous ConPTY redraw/echo on runners.
#[cfg(windows)]
const REAL_SHELL_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(windows)]
#[test]
fn powershell_narrow_draft_survives_conpty_redraw() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let root = tempfile::tempdir().unwrap();
    let mut config = super::store::SessionConfig::blank(super::store::SessionKind::Local);
    let local = config.local.as_mut().unwrap();
    local.shell = "powershell".into();
    local.working_directory = root.path().to_str().unwrap().into();
    config.meta.options.log_enabled = true;
    let id = term::open_tab_with_config(&config);
    assert!(super::term_net_tests::wait_until(
        REAL_SHELL_TIMEOUT,
        || {
            term::term_poll(id);
            let state = completion::terminal_completion_query(id);
            state.batch.context.ready && state.query.is_empty()
        }
    ));
    let metrics = super::term_diag::term_stats(id);
    let resize = |cols: usize| {
        term::term_resize_px(
            id,
            cols as f32 * metrics.cell_w as f32,
            8.0 * metrics.cell_h as f32,
        );
    };
    resize(30);
    let draft = "echo __NARROW_DRAFT_KEEP__";
    term::term_key(id, 0, 0, draft);
    let typed = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
        term::term_poll(id);
        completion::terminal_completion_query(id).query == draft
    });
    for cols in [24, 16, 36, 20] {
        let before = term::with_tab(id, |st| st.preview_revision);
        resize(cols);
        // ConPTY repaints the visible prompt line on resize without replaying
        // OSC B. Require that repaint to be delivered before checking the draft.
        let repainted = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
            term::term_poll(id);
            term::with_tab(id, |st| st.preview_revision) != before
        });
        let kept = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
            term::term_poll(id);
            completion::terminal_completion_query(id).query == draft
        });
        if !kept {
            let status = super::term_log::slog_status(id);
            super::term_log::slog_stop(id);
            super::term_net_tests::wait_until(Duration::from_secs(5), || {
                super::term_log::slog_pending_drains() == 0
            });
            let file = std::path::Path::new(&status.path).join("output.log");
            if let Ok(file) = std::fs::File::open(file) {
                let mut raw = Vec::new();
                file.take(65536).read_to_end(&mut raw).unwrap();
                println!(
                    "[conpty-narrow] cols={cols} output={:?}",
                    String::from_utf8_lossy(&raw)
                );
            }
        }
        term::with_tab(id, |st| {
            assert!(
                typed && repainted && kept,
                "cols={cols} typed={typed} repainted={repainted} active={} marked={} exact={:?} full={:?} legacy={:?} pending={} screen={:?}",
                st.pane.command_input_active(),
                st.pane.has_command_input_markers(),
                st.pane.command_input_prefix(),
                st.pane.command_input_line(),
                st.pane.command_prefix(),
                st.terminal_input.is_some(),
                st.pane.screen_text()
            );
        });
    }
    term::term_key(id, 0, 4, "c");
    assert!(super::term_net_tests::wait_until(
        REAL_SHELL_TIMEOUT,
        || {
            term::term_poll(id);
            let state = completion::terminal_completion_query(id);
            state.batch.context.ready && state.query.is_empty()
        }
    ));
    term::term_close_tab(id);
}

#[cfg(windows)]
#[test]
fn powershell_prompt_boundary_survives_conpty_redraw() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let root = tempfile::tempdir().unwrap();
    let mut config = super::store::SessionConfig::blank(super::store::SessionKind::Local);
    let local = config.local.as_mut().unwrap();
    local.shell = "powershell".into();
    local.working_directory = root.path().to_str().unwrap().into();
    config.meta.options.log_enabled = true;
    let id = term::open_tab_with_config(&config);
    let ready = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
        term::term_poll(id);
        let state = completion::terminal_completion_query(id);
        state.batch.context.ready && state.query.is_empty()
    });
    term::with_tab(id, |st| {
        assert!(
            ready,
            "startup alive={} active={} marked={} pending={} screen-len={} note-len={}",
            st.host.alive(),
            st.pane.command_input_active(),
            st.pane.has_command_input_markers(),
            st.terminal_input.is_some(),
            st.pane.screen_text().len(),
            st.note.borrow().len()
        );
    });
    let metrics = super::term_diag::term_stats(id);
    let mut redraws = Vec::new();
    for cols in [24, 40, 28, 60, 16, 30] {
        let before_revision = term::with_tab(id, |st| st.preview_revision);
        let before_active = term::with_tab(id, |st| st.pane.command_input_active());
        term::term_resize_px(
            id,
            cols as f32 * metrics.cell_w as f32,
            8.0 * metrics.cell_h as f32,
        );
        let after_resize = term::with_tab(id, |st| st.pane.command_input_active());
        let repainted = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
            term::term_poll(id);
            term::with_tab(id, |st| st.preview_revision) != before_revision
        });
        let ready = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
            term::term_poll(id);
            let state = completion::terminal_completion_query(id);
            state.batch.context.ready && state.query.is_empty()
        });
        redraws.push((
            cols,
            before_revision,
            term::with_tab(id, |st| st.preview_revision),
        ));
        if !ready || !repainted {
            let status = super::term_log::slog_status(id);
            super::term_log::slog_stop(id);
            super::term_net_tests::wait_until(Duration::from_secs(5), || {
                super::term_log::slog_pending_drains() == 0
            });
            let file = std::path::Path::new(&status.path).join("output.log");
            if let Ok(file) = std::fs::File::open(file) {
                let mut raw = Vec::new();
                file.take(65536).read_to_end(&mut raw).unwrap();
                println!(
                    "[conpty] isolated fixture output={:?}",
                    String::from_utf8_lossy(&raw)
                );
            }
        }
        let context = super::command_completion::completion_context(id);
        term::with_tab(id, |st| {
            assert!(
                ready && repainted,
                "cols={cols} repainted={repainted} lease-before={before_active} lease-after-reflow={after_resize} base-ready={} alive={} reading={} selected={} input-active={} pending={} shell={} marked={} exact={:?} full={:?} legacy={:?} input_line={:?} screen={:?}",
                context.ready, st.host.alive(), st.reading, st.pane.selection_count(), st.pane.command_input_active(), st.terminal_input.is_some(),
                super::completion_shell::configured_kind(st),
                st.pane.has_command_input_markers(),
                st.pane.command_input_prefix(),
                st.pane.command_input_line(),
                st.pane.command_prefix(),
                st.pane.input_line(),
                st.pane.screen_text()
            );
        });
    }
    let command = "echo __CANCELLED_DRAFT__";
    term::term_key(id, 0, 0, command);
    let typed = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
        term::term_poll(id);
        completion::terminal_completion_query(id).query == command
    });
    let state = completion::terminal_completion_query(id);
    term::with_tab(id, |st| {
        assert!(typed,
            "redraws={redraws:?} ready={} query-len={} active={} marked={} pending={} command-on-screen={} exact-len={} full-len={} legacy-len={}",
            state.batch.context.ready, state.query.len(), st.pane.command_input_active(),
            st.pane.has_command_input_markers(), st.terminal_input.is_some(),
            st.pane.screen_text().contains(command),
            st.pane.command_input_prefix().map_or(0, |s| s.len()),
            st.pane.command_input_line().map_or(0, |s| s.len()),
            st.pane.command_prefix().map_or(0, |s| s.len()));
    });
    term::term_key(id, 0, 4, "c");
    let cancelled = super::term_net_tests::wait_until(REAL_SHELL_TIMEOUT, || {
        term::term_poll(id);
        let state = completion::terminal_completion_query(id);
        state.batch.context.ready && state.query.is_empty()
    });
    term::with_tab(id, |st| {
        assert!(
            cancelled,
            "cancel active={} marked={} exact={:?} legacy={:?} pending={} screen={:?}",
            st.pane.command_input_active(),
            st.pane.has_command_input_markers(),
            st.pane.command_input_prefix(),
            st.pane.command_prefix(),
            st.terminal_input.is_some(),
            st.pane.screen_text()
        );
    });
    term::term_close_tab(id);
}
