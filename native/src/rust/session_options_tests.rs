//! Public option behavior at the real terminal/bridge boundary.
use super::store::{session_config::SessionConfig, SessionKind};
use super::{sftp, system_monitor, term, term_lifecycle};

#[test]
fn disabled_sftp_rejects_before_a_transport_is_needed() {
    let _guard = super::term_net_tests::net_serial();
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.host = "127.0.0.1".into();
    ssh.port = 1;
    ssh.connect_timeout_secs = 1;
    cfg.meta.sftp_enabled = false;
    let tab = term::open_tab_with_config(&cfg);
    let uid = term::term_uid(tab);
    sftp::sftp_open(&uid);
    let denied = sftp::take_ops()
        .into_iter()
        .find(|op| op.uid == uid)
        .unwrap();
    assert!(!denied.ok);
    assert!(denied.error.contains("关闭 SFTP"));
    // Negative control: enabled reaches the transport check instead of the option denial.
    super::term::with_tab(tab, |st| st.config.meta.sftp_enabled = true);
    sftp::sftp_open(&uid);
    let enabled = sftp::take_ops()
        .into_iter()
        .find(|op| op.uid == uid)
        .unwrap();
    assert!(!enabled.error.contains("关闭 SFTP"));
    assert!(!sftp::channel_ready(&uid));
    term::term_close_tab(tab);
    assert!(!system_monitor::context_system_snapshot(tab).enabled);
}

#[test]
fn local_monitor_samples_asynchronously_and_disappears_on_disconnect() {
    let _guard = super::term_net_tests::net_serial();
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    cfg.meta.sysmon_enabled = false;
    let tab = term::open_tab_with_config(&cfg);
    assert!(!system_monitor::context_system_snapshot(tab).enabled);
    super::term::with_tab(tab, |st| st.config.meta.sysmon_enabled = true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let sample = loop {
        let sample = system_monitor::context_system_snapshot(tab);
        if sample.ready {
            break sample;
        }
        assert!(sample.error.is_empty(), "{}", sample.error);
        assert!(
            std::time::Instant::now() < deadline,
            "no system sample delivered"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(sample.enabled && sample.total_bytes > 0);
    assert!(sample.used_bytes <= sample.total_bytes);
    term_lifecycle::term_disconnect_tab(tab);
    assert!(!system_monitor::context_system_snapshot(tab).enabled);
    term::term_close_tab(tab);
}

// Peer bytes are an independent oracle: configured terminal input and raw Hex
// must take different paths, while decoded output still reaches the real pane/log.
#[test]
fn configured_charset_keys_and_raw_sender_reach_the_tcp_peer() {
    use std::io::{Read, Write};
    use std::time::Duration;
    let _guard = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0u8; 16];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"\xd6\xd0\xce\xc4\x08\r\n\xe4\xb8\xad\x1bOA\x1bOq");
        stream.write_all(b"\xd6").unwrap();
        std::thread::sleep(Duration::from_millis(10));
        stream
            .write_all(b"\xd0\xce\xc4\r\n\x1b]2;untrusted\x07\x07\x1b[?1l\x1b>")
            .unwrap();
        let mut normal = [0; 17];
        stream.read_exact(&mut normal).unwrap();
        assert_eq!(&normal, b"\x1b[A1\x1b[3~\x7f\x08\x08\x1b[3;5~");
        stream.write_all(b"PEER-NORMAL-ACK").unwrap();
        let mut eof = [0; 1];
        assert_eq!(stream.read(&mut eof).unwrap(), 0);
    });
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    cfg.tcp.as_mut().unwrap().host = "127.0.0.1".into();
    cfg.tcp.as_mut().unwrap().port = port;
    cfg.meta.charset = "GBK".into();
    cfg.meta.options.backspace_del = false;
    cfg.meta.options.enter_mode = 2;
    cfg.meta.options.remote_title = false;
    cfg.meta.options.log_enabled = true;
    cfg.meta.options.bell_sound = true;
    cfg.meta.options.app_cursor = true;
    cfg.meta.options.app_keypad = true;
    let tab = term::open_tab_with_config(&cfg);
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(tab);
            term::with_tab(tab, |st| st.host.connected())
        }
    ));
    assert_eq!(term::term_key(tab, 0, 0, "中文"), "中文".as_bytes());
    assert_eq!(term::term_key(tab, 11, 0, ""), b"\x08");
    assert_eq!(term::term_key(tab, 14, 0, ""), b"\r\n");
    super::term_ops::term_write_sessions(&[tab], "中".as_bytes(), true);
    assert_eq!(term::term_key(tab, 1, 0, ""), b"\x1bOA");
    assert_eq!(term::term_key(tab, 0, 8, "1"), b"\x1bOq");
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(tab);
            super::term_diag::term_screen_text(tab).contains("中文")
                && term::with_tab(tab, |st| st.bell_pending)
        }
    ));
    assert_eq!(term::term_key(tab, 1, 0, ""), b"\x1b[A");
    assert_eq!(term::term_key(tab, 0, 8, "1"), b"1");
    assert_eq!(term::term_key(tab, 10, 0, ""), b"\x1b[3~");
    term::with_tab(tab, |st| st.config.meta.options.delete_mode = 1);
    assert_eq!(term::term_key(tab, 10, 0, ""), b"\x7f");
    assert_eq!(term::term_key(tab, 11, 0, ""), b"\x08");
    term::with_tab(tab, |st| st.config.meta.options.delete_mode = 2);
    assert_eq!(term::term_key(tab, 10, 0, ""), b"\x08");
    assert_eq!(term::term_key(tab, 10, 4, ""), b"\x1b[3;5~");
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(tab);
            super::term_diag::term_screen_text(tab).contains("PEER-NORMAL-ACK")
        }
    ));
    assert!(!super::term_diag::term_title(tab).contains("untrusted"));
    assert!(super::term_log::slog_active(tab));
    assert!(super::term_log::slog_recent(tab, 20).contains("中文"));
    assert!(super::session_advanced::context_take_bell(tab));
    assert!(!super::session_advanced::context_take_bell(tab));
    term::term_close_tab(tab);
    peer.join().unwrap();
}
