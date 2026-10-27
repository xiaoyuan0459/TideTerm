//! Keymap reference tests — assert [`encode_key`] turns representative key presses +
//! modifiers into the exact PTY byte sequence a VT/xterm shell expects.
//!
//! Modeled on Alacritty's keymap reference: a small, explicit table of (key, modifiers)
//! → bytes. Since Phase A (路线 A) the encoder speaks the neutral
//! [`KeyEvent`]; the tests construct it from the same codepoint text Slint delivers
//! (pinned here as literals, so a drift in either the table or the encoder shows up as
//! a byte-level diff). The PTY bytes asserted below are byte-for-byte the pre-Phase-A
//! ones.

use tiderm_terminal::{encode_key, Key, KeyEvent};

/// The `KeyEvent.text` Slint delivers for a special key (a private-use / control
/// codepoint, per slint's `internal/common/key_codes.rs` at the pinned rev).
fn special(text: &str, ctrl: bool, alt: bool, shift: bool) -> KeyEvent<'_> {
    KeyEvent::from_text(text, ctrl, alt, shift)
}

const UP: &str = "\u{F700}";
const DOWN: &str = "\u{F701}";
const LEFT: &str = "\u{F702}";
const RIGHT: &str = "\u{F703}";
const HOME: &str = "\u{F729}";
const END: &str = "\u{F72B}";
const PAGEUP: &str = "\u{F72C}";
const PAGEDOWN: &str = "\u{F72D}";
const DELETE: &str = "\u{7f}";
const RETURN: &str = "\u{a}";
const BACKSPACE: &str = "\u{8}";
const TAB: &str = "\u{9}";
const ESCAPE: &str = "\u{1b}";

#[test]
fn printables_classify_as_char_and_pass_through_unchanged() {
    let ev = KeyEvent::from_text("a", false, false, false);
    assert_eq!(ev.key, Key::Char('a'));
    assert_eq!(encode_key(&ev), Some(b"a".to_vec()));
    assert_eq!(
        encode_key(&KeyEvent::from_text("Z", false, false, true)),
        Some(b"Z".to_vec())
    );
    assert_eq!(
        encode_key(&special("7", false, false, false)),
        Some(b"7".to_vec())
    );
}

#[test]
fn arrow_keys_emit_csi_sequences() {
    assert_eq!(
        encode_key(&special(UP, false, false, false)),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        encode_key(&special(DOWN, false, false, false)),
        Some(b"\x1b[B".to_vec())
    );
    assert_eq!(
        encode_key(&special(RIGHT, false, false, false)),
        Some(b"\x1b[C".to_vec())
    );
    assert_eq!(
        encode_key(&special(LEFT, false, false, false)),
        Some(b"\x1b[D".to_vec())
    );
}

#[test]
fn page_and_home_end_keys_emit_their_sequences() {
    assert_eq!(
        encode_key(&special(PAGEUP, false, false, false)),
        Some(b"\x1b[5~".to_vec())
    );
    assert_eq!(
        encode_key(&special(PAGEDOWN, false, false, false)),
        Some(b"\x1b[6~".to_vec())
    );
    assert_eq!(
        encode_key(&special(HOME, false, false, false)),
        Some(b"\x1b[H".to_vec())
    );
    assert_eq!(
        encode_key(&special(END, false, false, false)),
        Some(b"\x1b[F".to_vec())
    );
    assert_eq!(
        encode_key(&special(DELETE, false, false, false)),
        Some(b"\x1b[3~".to_vec())
    );
}

#[test]
fn enter_tab_backspace_escape() {
    assert_eq!(
        encode_key(&special(RETURN, false, false, false)),
        Some(b"\r".to_vec())
    );
    assert_eq!(
        encode_key(&special(TAB, false, false, false)),
        Some(b"\t".to_vec())
    );
    // Terminals conventionally map Backspace to DEL (0x7f).
    assert_eq!(
        encode_key(&special(BACKSPACE, false, false, false)),
        Some(vec![0x7f])
    );
    assert_eq!(
        encode_key(&special(ESCAPE, false, false, false)),
        Some(vec![0x1b])
    );
}

#[test]
fn ctrl_letters_map_to_control_bytes() {
    // Ctrl-C -> ETX (0x03), Ctrl-D -> EOT (0x04), case-insensitive.
    assert_eq!(
        encode_key(&special("c", true, false, false)),
        Some(vec![0x03])
    );
    assert_eq!(
        encode_key(&special("C", true, false, false)),
        Some(vec![0x03])
    );
    assert_eq!(
        encode_key(&special("d", true, false, false)),
        Some(vec![0x04])
    );
    assert_eq!(
        encode_key(&special("a", true, false, false)),
        Some(vec![0x01])
    );
}

#[test]
fn ctrl_punctuation_maps_to_low_control_bytes() {
    assert_eq!(
        encode_key(&special(" ", true, false, false)),
        Some(vec![0x00])
    ); // Ctrl-Space -> NUL
    assert_eq!(
        encode_key(&special("[", true, false, false)),
        Some(vec![0x1b])
    ); // Ctrl-[ -> ESC
    assert_eq!(
        encode_key(&special("\\", true, false, false)),
        Some(vec![0x1c])
    );
    assert_eq!(
        encode_key(&special("]", true, false, false)),
        Some(vec![0x1d])
    );
}

#[test]
fn alt_prefixes_an_escape() {
    // Alt/Meta sends ESC then the text (e.g. Alt-b for word-back in readline).
    assert_eq!(
        encode_key(&special("b", false, true, false)),
        Some(vec![0x1b, b'b'])
    );
}

#[test]
fn empty_text_sends_nothing() {
    // A bare modifier press (no text) must not emit bytes.
    assert_eq!(encode_key(&special("", false, false, false)), None);
}
