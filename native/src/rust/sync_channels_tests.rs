//! Actual loopback bytes own routing; the Qt journey owns membership editing.
use super::sync_channels::*;
use super::term_net_tests::{net_serial, settle, tcp_cfg, wait_until, Sink};
use super::{term, term_diag, term_ops, term_reading};
use std::io::{ErrorKind, Read};
use std::time::Duration;

pub(super) struct FixtureSync(u64);
impl Drop for FixtureSync {
    fn drop(&mut self) {
        term_sync_close(self.0);
    }
}
pub(super) fn fixture(ids: &[u32]) -> FixtureSync {
    let owner = term_sync_create();
    assert_ne!(owner, 0);
    let guard = FixtureSync(owner);
    assert!(term_sync_update(owner, ids));
    let mut channels = term_sync_snapshot(owner);
    channels[0].members = ids.to_vec();
    assert_eq!(term_sync_replace(owner, &channels), 0);
    guard
}

fn drain(sinks: &[Sink], received: &mut [Vec<u8>]) {
    for (sink, received) in sinks.iter().zip(received) {
        for stream in sink.live.lock().unwrap().iter_mut() {
            let mut bytes = [0; 128];
            loop {
                match stream.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => received.extend_from_slice(&bytes[..count]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::WouldBlock
                                | ErrorKind::TimedOut
                                | ErrorKind::ConnectionReset
                        ) =>
                    {
                        break
                    }
                    result => panic!("loopback receive failed: {result:?}"),
                }
            }
        }
    }
}
fn expected(sinks: &[Sink], received: &mut [Vec<u8>], expected: &[Vec<u8>]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        drain(sinks, received);
        if received
            .iter()
            .zip(expected)
            .all(|(actual, wanted)| actual.len() >= wanted.len())
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "sync byte delivery timed out: {received:?} expected {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(60));
    drain(sinks, received);
    assert_eq!(
        received, expected,
        "input crossed a channel/window or was duplicated"
    );
}

#[test]
fn four_channels_route_only_owned_members_and_retire_with_the_window() {
    let _serial = net_serial();
    let _input = super::term_tests::input_lock_guard();
    let sinks: Vec<_> = (0..10).map(|_| Sink::start().unwrap()).collect();
    let ids: Vec<_> = sinks
        .iter()
        .map(|sink| term::open_tab_with_config(&tcp_cfg(sink.port)))
        .collect();
    assert!(wait_until(Duration::from_secs(5), || {
        for &id in &ids {
            settle(id);
        }
        ids.iter().all(|&id| term_diag::term_status_kind(id) == 2)
            && sinks.iter().all(|sink| sink.accepted() == 1)
    }));
    let a = fixture(&ids[..8]);
    let b = fixture(&ids[8..]);
    let mut channels = term_sync_snapshot(a.0);
    for (index, channel) in channels.iter_mut().enumerate() {
        channel.name = format!("channel-{index}");
        channel.members = ids[index * 2..index * 2 + 2].to_vec();
    }
    assert_eq!(term_sync_replace(a.0, &channels), 0);
    let mut other = term_sync_snapshot(b.0);
    other[0].name = channels[0].name.clone(); // Identical names never join windows.
    assert_eq!(term_sync_replace(b.0, &other), 0);
    assert!(
        !term_sync_update(b.0, &[ids[0], ids[9]]),
        "cannot steal a different window's view"
    );
    let mut received = vec![Vec::new(); ids.len()];
    let mut wanted = received.clone();
    for index in 0..4 {
        let bytes = format!("G{index}");
        term::term_key(ids[index * 2], 0, 0, &bytes);
        for item in &mut wanted[index * 2..index * 2 + 2] {
            item.extend_from_slice(bytes.as_bytes());
        }
        expected(&sinks, &mut received, &wanted);
    }
    channels[1].members.push(ids[0]);
    assert_eq!(term_sync_replace(a.0, &channels), 4);
    channels[1].members.pop();
    let name = channels[1].name.clone();
    channels[1].name = channels[0].name.clone();
    assert_eq!(term_sync_replace(a.0, &channels), 3);
    channels[1].name = name;
    assert_eq!(term_sync_status(a.0, ids[2]).name, channels[1].name);
    term_reading::term_set_reading(ids[1], true);
    term::term_key(ids[0], 0, 0, "R");
    wanted[0].push(b'R');
    expected(&sinks, &mut received, &wanted);
    term_reading::term_set_reading(ids[0], true);
    assert!(term::term_key(ids[0], 0, 0, "reader").is_empty());
    expected(&sinks, &mut received, &wanted);
    term_reading::term_set_reading(ids[0], false);
    term_reading::term_set_reading(ids[1], false);
    term_ops::term_set_locked(true);
    assert_eq!(term_sync_replace(a.0, &channels), 6);
    assert!(term::term_key(ids[0], 0, 0, "locked").is_empty());
    term_ops::term_set_locked(false);
    expected(&sinks, &mut received, &wanted);
    channels[0].members = vec![ids[0]];
    assert_eq!(term_sync_replace(a.0, &channels), 0);
    term::term_key(ids[0], 0, 0, "L");
    wanted[0].push(b'L');
    expected(&sinks, &mut received, &wanted);
    let old_uid = term::term_uid(ids[4]);
    assert!(super::term_lifecycle::term_reconnect_tab(ids[4]));
    assert!(wait_until(Duration::from_secs(5), || {
        settle(ids[4]);
        sinks[4].accepted() == 2 && term_diag::term_status_kind(ids[4]) == 2
    }));
    assert_ne!(term::term_uid(ids[4]), old_uid);
    term::term_key(ids[4], 0, 0, "N");
    wanted[4].push(b'N');
    wanted[5].push(b'N');
    expected(&sinks, &mut received, &wanted);
    term::term_close_tab(ids[4]);
    assert_eq!(term_sync_status(a.0, ids[5]).members, 1);
    assert_eq!(
        term_sync_replace(a.0, &channels),
        4,
        "closed members cannot be re-admitted"
    );
    drop(a);
    term::term_key(ids[6], 0, 0, "X");
    wanted[6].push(b'X');
    expected(&sinks, &mut received, &wanted);
    term::term_key(ids[8], 0, 0, "B");
    wanted[8].push(b'B');
    wanted[9].push(b'B');
    expected(&sinks, &mut received, &wanted);
    drop(b);
    for id in ids {
        term::term_close_tab(id);
    }
}
