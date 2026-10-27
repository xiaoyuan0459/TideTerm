//! Reading mode owns user-input isolation; real loopback peers are the byte oracle.
use super::{term, term_diag, term_ops};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn peer() -> (u32, TcpStream) {
    use super::store::{session_config::SessionConfig, SessionKind};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    let tcp = cfg.tcp.as_mut().unwrap();
    tcp.host = "127.0.0.1".into();
    tcp.port = listener.local_addr().unwrap().port();
    let tab = term::open_tab_with_config(&cfg);
    assert!(super::term_net_tests::wait_until(
        Duration::from_secs(5),
        || {
            term::term_poll(tab);
            term::with_tab(tab, |st| st.host.connected())
        }
    ));
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    (tab, stream)
}

fn received(stream: &mut TcpStream, expected: &[u8]) {
    let mut actual = vec![0; expected.len()];
    stream
        .read_exact(&mut actual)
        .unwrap_or_else(|error| panic!("waiting for {expected:?}: {error}; partial={actual:?}"));
    assert_eq!(actual, expected, "unexpected user bytes reached the peer");
}

fn silent(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    let mut bytes = [0; 128];
    match stream.read(&mut bytes) {
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        result => panic!(
            "reading mode must keep the connection open without user bytes: {result:?}, {bytes:?}"
        ),
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
}

// Regression: Ctrl+Enter previously emitted CR and the next browsing key leaked.
// Existing encoding tests do not exercise mode transitions or a sync recipient.
#[test]
fn reading_mode_keeps_identity_and_isolates_all_user_send_paths() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (a, mut pa) = peer();
    let (b, mut pb) = peer();
    let uid = term::term_uid(a);
    let protocol = term::protocol_of_uid(&uid).unwrap();
    let count = term::term_tab_count();
    let sync = super::sync_channels_tests::fixture(&[a, b]);
    assert!(
        term::term_key(a, 14, 4, "").is_empty(),
        "mode toggle sent a byte"
    );
    assert!(
        term::term_key(a, 0, 0, "browse").is_empty(),
        "ordinary browsing text leaked"
    );
    term::term_ime_commit(a, "输入");
    term::term_write(a, b"late-input");
    term_diag::term_feed_inject(a, b"\x1b[?1003h\x1b[?1006h");
    assert!(!term::term_mouse_send(a, 0, 0, 20.0, 20.0));
    assert!(term::term_mouse(a, 1, 0, 0, 20.0, 20.0).is_empty());
    assert!(term::term_wheel(a, 3, 20.0, 20.0).is_empty());
    term_diag::term_feed_inject(a, b"\x1b[?1049h\x1b[?1003l");
    assert!(term::term_wheel(a, -3, 20.0, 20.0).is_empty());
    assert_eq!(term::term_uid(a), uid);
    assert_eq!(term::term_tab_count(), count);
    assert!(std::sync::Arc::ptr_eq(
        &protocol,
        &term::protocol_of_uid(&uid).unwrap()
    ));
    assert_eq!(
        super::term_lifecycle::active_config(a).kind,
        super::store::SessionKind::Tcp
    );
    // The other pane remains writable; synchronization must skip the reader.
    assert_eq!(term::term_key(b, 0, 0, "B"), b"B");
    received(&mut pb, b"B");
    silent(&mut pa);
    pa.write_all(b"INBOUND-READING\r\n").unwrap();
    assert!(
        super::term_net_tests::wait_until(Duration::from_secs(3), || {
            term::term_poll(a);
            term_diag::term_screen_text(a).contains("INBOUND-READING")
        }),
        "output pump stopped in reading mode"
    );
    assert!(term::term_key(a, 14, 4, "").is_empty());
    assert_eq!(term::term_key(a, 0, 0, "A"), b"A");
    received(&mut pa, b"A");
    received(&mut pb, b"A");
    drop(sync);
    assert!(term::term_key(a, 14, 4, "").is_empty());
    term_ops::term_write_sessions(&[a, b], b"targets", true);
    received(&mut pb, b"targets");
    assert_eq!(term_ops::term_write_sessions(&[a, b], b"all", false), 1);
    received(&mut pb, b"all");
    silent(&mut pa);
    assert!(!super::term_reading::term_paste(a, b""));
    assert!(super::term_reading::term_reading(a));
    let paste = b"\x1b[200~explicit-paste\x1b[201~";
    assert!(super::term_reading::term_paste(a, paste));
    assert!(!super::term_reading::term_reading(a));
    received(&mut pa, paste);
    silent(&mut pa);
    silent(&mut pb);
    term::term_close_tab(a);
    term::term_close_tab(b);
}

// The peer is the transport oracle; cursor columns and selected text are the
// observable reading result, independent of the command parser's representation.
#[test]
fn local_navigation_moves_a_reading_caret_without_sending_bytes() {
    let _serial = super::term_net_tests::net_serial();
    let _input = super::term_tests::input_lock_guard();
    let (tab, mut stream) = peer();
    super::term_actions::term_clear(tab, 2);
    term_diag::term_feed_inject(tab, b"\x1b[Halpha.beta second\r\nnext sentence.");
    assert!(super::term_reading::term_set_reading(tab, true));
    term::term_key(tab, 0, 0, "0");
    assert_eq!(
        term_diag::term_stats(tab).cursor_col,
        0,
        "Local has no reading caret"
    );
    term::term_key(tab, 0, 0, "3");
    term::term_key(tab, 0, 0, "l");
    assert_eq!(term_diag::term_stats(tab).cursor_col, 3);
    silent(&mut stream);
    for ch in "gg0\"ay$".chars() {
        term::term_key(tab, 0, 0, &ch.to_string());
    }
    assert_eq!(crate::term_selected_text(tab), "alpha.beta second");
    for ch in "\"aP".chars() {
        term::term_key(tab, 0, 0, &ch.to_string());
    }
    received(&mut stream, b"alpha.beta second");
    assert!(super::term_reading::term_reading(tab));
    silent(&mut stream);
    for ch in "\"ap".chars() {
        term::term_key(tab, 0, 0, &ch.to_string());
    }
    received(&mut stream, b"alpha.beta second");
    assert!(!super::term_reading::term_reading(tab));
    silent(&mut stream);
    term::term_close_tab(tab);
}
