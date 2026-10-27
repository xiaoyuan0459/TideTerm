//! Native FIFO/charset/input/lifecycle risks beyond the core protocol tests.
use super::store::{session_config::SessionConfig, SessionKind};
use super::{modem, term, term_diag, term_lifecycle};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
#[test]
fn modem_uses_raw_session_bytes_blocks_keys_and_restores_trailing_terminal_output() {
    let _guard = super::term_net_tests::net_serial();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.bin");
    let payload = [255, 254, 0, 128, 13, 10, 24];
    std::fs::write(&path, payload).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (handshake, ready) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        stream.write_all(b"C").unwrap();
        let mut packet = [0; 133];
        stream.read_exact(&mut packet).unwrap();
        assert_eq!(&packet[..3], &[1, 1, 254]);
        assert_eq!(&packet[3..10], &payload);
        stream.write_all(&[6]).unwrap();
        let mut eot = [0];
        stream.read_exact(&mut eot).unwrap();
        assert_eq!(eot, [4]);
        stream.write_all(b"\x06AFTER_MODEM").unwrap();
        let mut eof = [0];
        assert_eq!(stream.read(&mut eof).unwrap(), 0);
    });
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    let tcp = cfg.tcp.as_mut().unwrap();
    tcp.host = "127.0.0.1".into();
    tcp.port = port;
    tcp.data_mode = 1;
    cfg.meta.charset = "GBK".into();
    let tab = term::open_tab_with_config(&cfg);
    let deadline = Instant::now() + Duration::from_secs(5);
    while term_diag::term_status_kind(tab) != 2 {
        term::term_poll(tab);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let error = modem::modem_start(tab, 0, false, vec![path.to_string_lossy().into()]);
    assert!(error.is_empty(), "{error}");
    assert!(term::term_key(tab, 0, 0, "intrude").is_empty());
    term::term_write(tab, b"intrude");
    // Hold the peer before its first control byte until input blocking has been
    // checked. A localhost transfer can otherwise finish before these calls.
    handshake.send(()).unwrap();
    while modem::modem_progress(tab).busy {
        term::term_poll(tab);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let progress = modem::modem_progress(tab);
    assert!(progress.finished);
    assert!(progress.error.is_empty(), "{}", progress.error);
    assert_eq!(progress.bytes, 7);
    while !term_diag::term_screen_text(tab).contains("AFTER_MODEM") {
        term::term_poll(tab);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    term_lifecycle::term_disconnect_tab(tab);
    term::term_close_tab(tab);
    assert!(modem::modem_progress(tab).finished);
    peer.join().unwrap();
}

#[test]
fn closing_a_tab_during_receive_removes_partial_download_and_closes_peer() {
    let _guard = super::term_net_tests::net_serial();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("interrupted.bin");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut control = [0];
        stream.read_exact(&mut control).unwrap();
        assert_eq!(control, [b'C']);
        // One independently formed CRC block: the CRC of 128 zero bytes is zero.
        let mut packet = [0; 133];
        packet[..3].copy_from_slice(&[1, 1, 254]);
        stream.write_all(&packet).unwrap();
        stream.read_exact(&mut control).unwrap();
        assert_eq!(control, [6]);
        let mut tail = Vec::new();
        stream.read_to_end(&mut tail).unwrap();
        assert!(tail.is_empty() || tail == [24; 8]);
    });
    let mut cfg = SessionConfig::blank(SessionKind::Tcp);
    let tcp = cfg.tcp.as_mut().unwrap();
    tcp.host = "127.0.0.1".into();
    tcp.port = port;
    tcp.data_mode = 1;
    let tab = term::open_tab_with_config(&cfg);
    let deadline = Instant::now() + Duration::from_secs(5);
    while term_diag::term_status_kind(tab) != 2 {
        term::term_poll(tab);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let error = modem::modem_start(tab, 0, true, vec![target.to_string_lossy().into()]);
    assert!(error.is_empty(), "{error}");
    while modem::modem_progress(tab).bytes < 128 {
        term::term_poll(tab);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    term::term_close_tab(tab);
    assert!(modem::modem_progress(tab).finished);
    peer.join().unwrap();
    while std::fs::read_dir(dir.path()).unwrap().next().is_some() {
        assert!(
            Instant::now() < deadline,
            "partial download survived tab closure"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!target.exists());
}
