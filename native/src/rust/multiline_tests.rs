//! Real loopback bytes own conversion/routing proof; Qt owns dialog and focus journeys.
use super::{multiline::*, term, term_diag, term_ops, term_reading};
use crate::bridge::multiline::multiline_ffi::SendOptions;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

struct Peer {
    tab: u32,
    stream: TcpStream,
    listener: TcpListener,
}
impl Peer {
    fn new() -> Self {
        use super::store::{session_config::SessionConfig, SessionKind};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut cfg = SessionConfig::blank(SessionKind::Tcp);
        let tcp = cfg.tcp.as_mut().unwrap();
        tcp.host = "127.0.0.1".into();
        tcp.port = listener.local_addr().unwrap().port();
        let tab = term::open_tab_with_config(&cfg);
        Self::wait(tab);
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Self {
            tab,
            stream,
            listener,
        }
    }
    fn wait(tab: u32) {
        assert!(super::term_net_tests::wait_until(
            Duration::from_secs(5),
            || {
                term::term_poll(tab);
                term::with_tab(tab, |st| st.host.alive())
            }
        ));
    }
    fn bytes(&mut self, expected: &[u8]) {
        let mut actual = vec![0; expected.len()];
        self.stream.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }
    fn silent(&mut self) {
        self.stream
            .set_read_timeout(Some(Duration::from_millis(120)))
            .unwrap();
        let mut bytes = [0; 128];
        assert!(
            matches!(self.stream.read(&mut bytes), Err(e) if matches!(e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)),
            "unexpected send"
        );
        self.stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        term::term_close_tab(self.tab);
    }
}

fn opts(eol: u8, tabs: u8, append: bool, paced: bool) -> SendOptions {
    SendOptions {
        eol,
        tabs,
        append,
        paced,
    }
}

#[test]
fn explicit_multiline_bytes_are_converted_once_and_never_expand_through_sync() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let mut a = Peer::new();
    let mut b = Peer::new();
    let mut other = Peer::new();
    let sync = super::sync_channels_tests::fixture(&[a.tab, b.tab, other.tab]);
    for (eol, expected) in [
        (0, "中  a\r\r尾\r"),
        (1, "中  a\n\n尾\n"),
        (2, "中  a\r\n\r\n尾\r\n"),
    ] {
        let token = multiline_begin(a.tab, &[a.tab, b.tab, a.tab], 0);
        assert_ne!(token, 0);
        let options = opts(eol, 2, true, false);
        assert_eq!(
            multiline_prepare(a.tab, token, "中\ta\r\n\r尾", &options).error,
            0
        );
        a.silent();
        b.silent();
        let result = multiline_step(a.tab, token);
        assert_eq!((result.sent, result.total, result.error), (1, 1, 0));
        a.bytes(expected.as_bytes());
        b.bytes(expected.as_bytes());
        assert_ne!(multiline_step(a.tab, token).error, 0);
        a.silent();
        b.silent();
        other.silent();
    }
    // Aggregate paste honors bracket mode and only explicit confirmation exits reading mode.
    term_diag::term_feed_inject(a.tab, b"\x1b[?2004h");
    term_reading::term_set_reading(a.tab, true);
    let token = multiline_begin(a.tab, &[a.tab], 1);
    assert_ne!(token, 0);
    assert_eq!(
        multiline_prepare(a.tab, token, "one\ntwo", &opts(0, 0, false, false)).error,
        0
    );
    assert!(term_reading::term_reading(a.tab));
    a.silent();
    assert_eq!(multiline_step(a.tab, token).error, 0);
    a.bytes(b"\x1b[200~one\rtwo\x1b[201~");
    assert!(!term_reading::term_reading(a.tab));
    other.silent();
    drop(sync);
}

#[test]
fn paced_plan_stops_on_cancel_lock_reading_reconnect_and_closed_targets() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let mut a = Peer::new();
    let mut b = Peer::new();
    let options = opts(0, 0, true, true);
    let arm = |source, targets: &[u32]| {
        let token = multiline_begin(source, targets, 0);
        assert_ne!(token, 0);
        assert_eq!(
            multiline_prepare(source, token, "first\nsecond", &options).error,
            0
        );
        token
    };
    let token = arm(a.tab, &[b.tab]);
    assert_ne!(
        multiline_prepare(a.tab, token, "replacement", &options).error,
        0
    );
    assert_eq!(multiline_step(a.tab, token).sent, 1);
    b.bytes(b"first\r");
    a.silent();
    multiline_end(a.tab, token);
    assert_ne!(multiline_step(a.tab, token).error, 0);
    b.silent();
    let token = arm(a.tab, &[b.tab]);
    term_ops::term_set_locked(true);
    term_ops::term_set_locked(false);
    assert_ne!(multiline_step(a.tab, token).error, 0);
    b.silent();
    let token = arm(a.tab, &[b.tab]);
    term_reading::term_set_reading(b.tab, true);
    term_reading::term_set_reading(b.tab, false);
    assert_ne!(multiline_step(a.tab, token).error, 0);
    b.silent();
    let token = arm(a.tab, &[b.tab]);
    assert!(super::term_lifecycle::term_reconnect_tab(b.tab));
    Peer::wait(b.tab);
    let (stream, _) = b.listener.accept().unwrap();
    b.stream = stream;
    assert_ne!(multiline_step(a.tab, token).error, 0);
    b.silent();
    let token = arm(a.tab, &[b.tab]);
    term::term_close_tab(b.tab);
    assert_ne!(multiline_step(a.tab, token).error, 0);
    a.silent();
    let token = arm(a.tab, &[a.tab]);
    term::term_close_tab(a.tab);
    assert!(!multiline_valid(a.tab, token));
    assert_ne!(multiline_step(a.tab, token).error, 0);
}

#[test]
fn malformed_or_oversized_edits_never_arm_a_send() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let mut peer = Peer::new();
    let token = multiline_begin(peer.tab, &[peer.tab], 0);
    for (text, options, error) in [
        (String::new(), opts(0, 0, false, false), 2),
        ("x".repeat((64 << 10) + 1), opts(0, 0, false, false), 3),
        ("\t".repeat(9000), opts(0, 8, false, false), 3),
        ("x\n".repeat(2049), opts(0, 0, false, true), 3),
        ("x".into(), opts(3, 0, false, false), 4),
    ] {
        assert_eq!(multiline_preview(&text, &options).error, error);
        assert_eq!(
            multiline_prepare(peer.tab, token, &text, &options).error,
            error
        );
    }
    let preview = multiline_preview("a\tb\nc", &opts(2, 0, false, false));
    assert_eq!(
        (preview.text.as_str(), preview.bytes, preview.lines),
        ("a\\tb\\r\\n\nc\n", 6, 2)
    );
    peer.silent();
    multiline_end(peer.tab, token);
}
