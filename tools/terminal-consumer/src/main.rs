//! Downstream compilation contract: no consumer-owned Cargo patches.
fn main() {
    let input = tiderm_terminal::encode_key(&tiderm_terminal::KeyEvent::from_text(
        "c", true, false, false,
    ));
    assert_eq!(input.as_deref(), Some(b"\x03".as_slice()));
}
