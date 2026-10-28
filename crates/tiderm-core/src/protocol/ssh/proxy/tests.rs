use super::*;
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn socks_authenticated_domain_connect_preserves_following_banner() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut greeting = [0; 3];
        peer.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 1, 2]);
        peer.write_all(&[5, 2]).unwrap();
        let mut auth = [0; 12];
        peer.read_exact(&mut auth).unwrap();
        assert_eq!(&auth, b"\x01\x04user\x05token");
        peer.write_all(&[1, 0]).unwrap();
        let mut request = [0; 19];
        peer.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"\x05\x01\x00\x03\x0cinternal.ssh\x00\x16");
        peer.write_all(b"\x05\x00\x00\x01\x7f\x00\x00\x01\x00\x16SSH-2.0-fixture\r\n")
            .unwrap();
    });
    let mut tcp = TcpStream::connect(address).unwrap();
    let proxy = ProxyOptions {
        kind: 1,
        username: "user".into(),
        password: "token".into(),
        ..Default::default()
    };
    negotiate(
        &mut tcp,
        &proxy,
        "internal.ssh",
        22,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let mut banner = String::new();
    tcp.read_to_string(&mut banner).unwrap();
    assert_eq!(banner, "SSH-2.0-fixture\r\n");
    worker.join().unwrap();
}

#[test]
fn http_connect_authentication_preserves_banner_and_rejects_error_status() {
    for (status, success) in [(200, true), (407, false)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                peer.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert_eq!(String::from_utf8(request).unwrap(), "CONNECT [::1]:22 HTTP/1.1\r\nHost: [::1]:22\r\nProxy-Authorization: Basic dXNlcjp0b2tlbg==\r\n\r\n");
            peer.write_all(
                format!("HTTP/1.1 {status} fixture\r\n\r\nSSH-2.0-fixture\r\n").as_bytes(),
            )
            .unwrap();
        });
        let mut tcp = TcpStream::connect(address).unwrap();
        let proxy = ProxyOptions {
            kind: 2,
            username: "user".into(),
            password: "token".into(),
            ..Default::default()
        };
        let result = negotiate(
            &mut tcp,
            &proxy,
            "::1",
            22,
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(result.is_ok(), success);
        if success {
            let mut banner = String::new();
            tcp.read_to_string(&mut banner).unwrap();
            assert_eq!(banner, "SSH-2.0-fixture\r\n");
        } else {
            assert!(result.unwrap_err().to_string().contains("407"));
        }
        worker.join().unwrap();
    }
}

#[test]
fn stalled_proxy_is_bounded_and_credentials_are_not_serialized() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        let mut greeting = [0; 3];
        peer.read_exact(&mut greeting).unwrap();
        std::thread::sleep(Duration::from_millis(300));
    });
    let mut tcp = TcpStream::connect(address).unwrap();
    let proxy = ProxyOptions {
        kind: 1,
        password: "never-persist-proxy-secret".into(),
        ..Default::default()
    };
    let started = Instant::now();
    assert!(negotiate(
        &mut tcp,
        &proxy,
        "host",
        22,
        started + Duration::from_millis(100)
    )
    .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!serde_json::to_string(&proxy)
        .unwrap()
        .contains("never-persist"));
    assert!(!format!("{proxy:?}").contains("never-persist"));
    worker.join().unwrap();
}
