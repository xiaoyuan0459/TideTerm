//! Public transfer entry points over real TCP, plus independent wire fixtures.
use super::*;
use crate::protocol::{Protocol, TcpConfig, TcpInstance};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn config(port: u16) -> TcpConfig {
    TcpConfig {
        host: "127.0.0.1".into(),
        port,
        terminal_type: "xterm".into(),
        connect_timeout_secs: 2,
        data_mode: "binary".into(),
    }
}
async fn pump(proto: Arc<dyn Protocol>, router: Router) {
    loop {
        let bytes = proto.read().await.unwrap();
        if bytes.is_empty() {
            router.eof();
            std::future::pending::<()>().await;
        }
        router.route(bytes).await;
    }
}
#[tokio::test]
async fn all_protocols_transfer_binary_files_over_tcp_with_exclusive_routes() {
    for kind in [Kind::X, Kind::Y, Kind::Z] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("payload.bin");
        // Includes every byte, a full block-number wrap and a short final block.
        let payload: Vec<u8> = (0..263_175).map(|n| (n % 256) as u8).collect();
        std::fs::write(&source, &payload).unwrap();
        let empty = dir.path().join("empty.bin");
        std::fs::write(&empty, []).unwrap();
        let target_dir = dir.path().join("received");
        std::fs::create_dir(&target_dir).unwrap();
        let target = target_dir.join("payload.bin");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let bridge = tokio::spawn(async move {
            let (mut a, _) = listener.accept().await.unwrap();
            let (mut b, _) = listener.accept().await.unwrap();
            tokio::io::copy_bidirectional(&mut a, &mut b).await.unwrap();
        });
        let a: Arc<dyn Protocol> =
            Arc::new(TcpInstance::connect(config(port), 80, 24).await.unwrap());
        let b: Arc<dyn Protocol> =
            Arc::new(TcpInstance::connect(config(port), 80, 24).await.unwrap());
        let send = Router::default();
        let receive = Router::default();
        let send_job = send
            .reserve(
                Request::Send {
                    kind,
                    paths: if matches!(kind, Kind::X) {
                        vec![source.clone()]
                    } else {
                        vec![source.clone(), empty]
                    },
                },
                1,
            )
            .unwrap();
        let receive_job = receive
            .reserve(
                Request::Receive {
                    kind,
                    path: if matches!(kind, Kind::X) {
                        target.clone()
                    } else {
                        target_dir.clone()
                    },
                },
                1,
            )
            .unwrap();
        assert!(send
            .reserve(
                Request::Send {
                    kind,
                    paths: vec![source]
                },
                1
            )
            .is_err());
        let jobs = async {
            tokio::join!(receive_job.run(b.clone()), send_job.run(a.clone()));
        };
        let readers = async {
            tokio::join!(
                pump(a.clone(), send.clone()),
                pump(b.clone(), receive.clone())
            );
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            tokio::select! { _ = jobs => {}, _ = readers => panic!("early EOF") }
        })
        .await
        .unwrap();
        assert!(
            send.progress().error.is_empty(),
            "{kind:?}: send {} ({} B), receive {} ({} B)",
            send.progress().error,
            send.progress().bytes,
            receive.progress().error,
            receive.progress().bytes
        );
        assert!(
            receive.progress().error.is_empty(),
            "{kind:?}: {}",
            receive.progress().error
        );
        let downloaded = std::fs::read(target).unwrap();
        assert_eq!(&downloaded[..payload.len()], payload);
        if !matches!(kind, Kind::X) {
            assert_eq!(downloaded.len(), payload.len());
            assert_eq!(
                std::fs::read(target_dir.join("empty.bin")).unwrap().len(),
                0
            );
        } else {
            assert!(downloaded[payload.len()..].iter().all(|b| *b == 0x1a));
        }
        drop(send_job);
        drop(receive_job);
        assert!(!send.busy());
        assert!(!receive.busy());
        let restored = receive.route(b"prompt>".to_vec()).await.unwrap();
        // ZMODEM's final hex header can leave its printable CRLF terminator.
        assert!(restored == b"prompt>" || (matches!(kind, Kind::Z) && restored == b"\r\nprompt>"));
        a.disconnect().await.unwrap();
        b.disconnect().await.unwrap();
        bridge.await.unwrap();
    }
}
#[tokio::test]
async fn xmodem_receive_negotiates_legacy_checksum_peer() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("legacy.bin");
    let payload = [0x91u8; 128];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        for _ in 0..3 {
            assert_eq!(socket.read_u8().await.unwrap(), b'C');
        }
        assert_eq!(socket.read_u8().await.unwrap(), 21);
        let mut packet = vec![1, 1, 254];
        packet.extend(payload);
        packet.push(
            payload
                .iter()
                .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
        );
        socket.write_all(&packet).await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 6);
        socket.write_all(&[4]).await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 6);
    });
    let proto: Arc<dyn Protocol> =
        Arc::new(TcpInstance::connect(config(port), 80, 24).await.unwrap());
    let router = Router::default();
    let job = router
        .reserve(
            Request::Receive {
                kind: Kind::X,
                path: target.clone(),
            },
            1,
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        tokio::select! {_ = job.run(proto.clone()) => {}, _ = pump(proto.clone(), router.clone()) => panic!("early EOF")}
    }).await.unwrap();
    assert!(
        router.progress().error.is_empty(),
        "{}",
        router.progress().error
    );
    assert_eq!(std::fs::read(target).unwrap(), payload);
    peer.await.unwrap();
    proto.disconnect().await.unwrap();
}
#[tokio::test]
async fn xmodem_retries_identical_crc_block_after_nak_then_finishes() {
    assert_eq!(xy::crc(b"123456789"), 0x31c3); // CRC-16/XMODEM reference vector.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.bin");
    std::fs::write(&path, b"binary\0\xff").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all(b"C").await.unwrap();
        let mut first = vec![0; 133];
        socket.read_exact(&mut first).await.unwrap();
        assert_eq!(&first[..3], &[1, 1, 254]);
        assert_eq!(&first[3..11], b"binary\0\xff");
        // Independent Python binascii.crc_hqx reference for this padded block.
        assert_eq!(&first[131..], &[0xeb, 0x03]);
        socket.write_all(&[21]).await.unwrap();
        let mut retry = vec![0; 133];
        socket.read_exact(&mut retry).await.unwrap();
        assert_eq!(first, retry);
        socket.write_all(&[6]).await.unwrap();
        assert_eq!(socket.read_u8().await.unwrap(), 4);
        socket.write_all(&[6]).await.unwrap();
    });
    let proto: Arc<dyn Protocol> =
        Arc::new(TcpInstance::connect(config(port), 80, 24).await.unwrap());
    let router = Router::default();
    let job = router
        .reserve(
            Request::Send {
                kind: Kind::X,
                paths: vec![path],
            },
            1,
        )
        .unwrap();
    let reader = pump(proto.clone(), router.clone());
    tokio::select! { _ = job.run(proto.clone()) => {}, _ = reader => panic!("reader ended early") }
    assert!(
        router.progress().error.is_empty(),
        "{}",
        router.progress().error
    );
    peer.await.unwrap();
    proto.disconnect().await.unwrap();
}
#[tokio::test]
async fn cancellation_releases_route_and_partial_file_without_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("partial.bin");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let proto: Arc<dyn Protocol> =
        Arc::new(TcpInstance::connect(config(port), 80, 24).await.unwrap());
    let (mut socket, _) = listener.accept().await.unwrap();
    let router = Router::default();
    let clone = router.clone();
    let job = router
        .reserve(
            Request::Receive {
                kind: Kind::X,
                path: target.clone(),
            },
            10,
        )
        .unwrap();
    let reader = tokio::spawn(pump(proto.clone(), router.clone()));
    let run = tokio::spawn(async move {
        job.run(proto.clone()).await;
        drop(job);
        proto.disconnect().await.unwrap();
    });
    assert_eq!(socket.read_u8().await.unwrap(), b'C');
    let payload = [0x81; 128];
    let mut packet = vec![1, 1, 254];
    packet.extend(payload);
    packet.extend(xy::crc(&payload).to_be_bytes());
    socket.write_all(&packet).await.unwrap();
    assert_eq!(socket.read_u8().await.unwrap(), 6);
    assert_eq!(router.progress().bytes, 128);
    clone.cancel();
    let mut can = [0; 8];
    socket.read_exact(&mut can).await.unwrap();
    assert_eq!(can, [24; 8]);
    tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .unwrap()
        .unwrap();
    reader.abort();
    assert!(!router.busy());
    assert_eq!(router.progress().error, "transfer cancelled");
    assert!(!target.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let existing = dir.path().join("existing.bin");
    std::fs::write(&existing, b"keep").unwrap();
    assert!(files::Download::new(&existing, None).is_err());
    assert_eq!(std::fs::read(existing).unwrap(), b"keep");
    for name in ["../evil", "C:\\evil", "dir/file", "NUL.txt", "dot.", "a\0b"] {
        assert!(files::safe_name(name.as_bytes()).is_err());
    }
}
