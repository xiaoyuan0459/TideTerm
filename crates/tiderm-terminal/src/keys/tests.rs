use super::*;

#[test]
fn functional_keys_do_not_require_synthetic_text() {
    for (key, expected) in [
        (Key::Up, b"\x1b[A".as_slice()),
        (Key::Return, b"\r".as_slice()),
        (Key::F(1), b"\x1bOP".as_slice()),
    ] {
        assert_eq!(
            encode_key(&KeyEvent {
                key,
                text: "",
                mods: Modifiers::default()
            })
            .as_deref(),
            Some(expected)
        );
    }
}

/// The text Slint delivers for the special keys — the same codepoints
/// [`Key::classify`] maps (kept as literals so the tests pin the exact bytes).
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
const BACKTAB: &str = "\u{19}";
const ESCAPE: &str = "\u{1b}";
const F5: &str = "\u{F708}";
const F1: &str = "\u{F704}";
const F4: &str = "\u{F707}";
const F12: &str = "\u{F70F}";
const F10: &str = "\u{F70D}";
const CONTROL: &str = "\u{11}";
const SHIFT: &str = "\u{10}";
const ALT: &str = "\u{12}";

fn kev(text: &str, ctrl: bool, alt: bool, shift: bool) -> KeyEvent<'_> {
    KeyEvent {
        key: Key::classify(text),
        text,
        mods: Modifiers {
            ctrl,
            alt,
            shift,
            meta: false,
        },
    }
}

#[test]
fn classify_maps_the_codepoint_table() {
    assert_eq!(Key::classify(UP), Key::Up);
    assert_eq!(Key::classify(DOWN), Key::Down);
    assert_eq!(Key::classify(LEFT), Key::Left);
    assert_eq!(Key::classify(RIGHT), Key::Right);
    assert_eq!(Key::classify(HOME), Key::Home);
    assert_eq!(Key::classify(END), Key::End);
    assert_eq!(Key::classify(PAGEUP), Key::PageUp);
    assert_eq!(Key::classify(PAGEDOWN), Key::PageDown);
    assert_eq!(Key::classify(DELETE), Key::Delete);
    assert_eq!(Key::classify(RETURN), Key::Return);
    assert_eq!(Key::classify(BACKSPACE), Key::Backspace);
    assert_eq!(Key::classify(TAB), Key::Tab);
    assert_eq!(Key::classify(BACKTAB), Key::Backtab);
    assert_eq!(Key::classify(ESCAPE), Key::Escape);
    assert_eq!(Key::classify(F5), Key::F(5));
    // F13+ and bare modifiers stay Unhandled (raw text passthrough, as before).
    assert_eq!(Key::classify("\u{F710}"), Key::Unhandled);
    assert_eq!(Key::classify(CONTROL), Key::Unhandled);
    assert_eq!(Key::classify(SHIFT), Key::Unhandled);
    assert_eq!(Key::classify(ALT), Key::Unhandled);
    // Unknown private-use codepoints are Unhandled (is_printable excludes them).
    assert_eq!(Key::classify("\u{E000}"), Key::Unhandled);
    assert_eq!(Key::classify(""), Key::Unhandled);
    // Printables classify as Char.
    assert_eq!(Key::classify("a"), Key::Char('a'));
    assert_eq!(Key::classify("世"), Key::Char('世'));
    assert_eq!(Key::classify(" "), Key::Char(' '));
}

#[test]
fn plain_text_passes_through() {
    assert_eq!(
        encode_key(&kev("a", false, false, false)),
        Some(b"a".to_vec())
    );
    assert_eq!(
        encode_key(&kev("A", false, false, true)),
        Some(b"A".to_vec())
    );
    assert_eq!(
        encode_key(&kev("5", false, false, false)),
        Some(b"5".to_vec())
    );
}

#[test]
fn enter_and_backspace_and_tab() {
    assert_eq!(
        encode_key(&kev(RETURN, false, false, false)),
        Some(b"\r".to_vec())
    );
    assert_eq!(
        encode_key(&kev(BACKSPACE, false, false, false)),
        Some(vec![0x7f])
    );
    assert_eq!(
        encode_key(&kev(TAB, false, false, false)),
        Some(b"\t".to_vec())
    );
}

#[test]
fn shift_tab_is_backtab() {
    // Slint's normal delivery: the dedicated Backtab key (with or without the
    // shift flag set).
    assert_eq!(
        encode_key(&kev(BACKTAB, false, false, true)),
        Some(b"\x1b[Z".to_vec())
    );
    assert_eq!(
        encode_key(&kev(BACKTAB, false, false, false)),
        Some(b"\x1b[Z".to_vec())
    );
    // Defensive: a backend that reports Tab + shift modifier instead.
    assert_eq!(
        encode_key(&kev(TAB, false, false, true)),
        Some(b"\x1b[Z".to_vec())
    );
}

#[test]
fn arrows_emit_csi() {
    assert_eq!(
        encode_key(&kev(UP, false, false, false)),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        encode_key(&kev(LEFT, false, false, false)),
        Some(b"\x1b[D".to_vec())
    );
}

#[test]
fn ctrl_c_is_etx() {
    assert_eq!(encode_key(&kev("c", true, false, false)), Some(vec![0x03]));
    assert_eq!(encode_key(&kev("C", true, false, false)), Some(vec![0x03]));
    assert_eq!(encode_key(&kev("d", true, false, false)), Some(vec![0x04]));
}

#[test]
fn alt_prefixes_escape() {
    assert_eq!(
        encode_key(&kev("b", false, true, false)),
        Some(vec![0x1b, b'b'])
    );
}

#[test]
fn empty_text_sends_nothing() {
    assert_eq!(encode_key(&kev("", false, false, false)), None);
}

#[test]
fn plain_pageup_down_still_reach_the_shell() {
    // Without Shift, PageUp/PageDown encode to their CSI sequences (unchanged behavior).
    assert_eq!(
        encode_key(&kev(PAGEUP, false, false, false)),
        Some(b"\x1b[5~".to_vec())
    );
    assert_eq!(
        encode_key(&kev(PAGEDOWN, false, false, false)),
        Some(b"\x1b[6~".to_vec())
    );
}

#[test]
fn shift_pageup_down_are_gated_from_the_pty() {
    // The scrollback gesture must never leak bytes to the shell.
    assert_eq!(encode_key(&kev(PAGEUP, false, false, true)), None);
    assert_eq!(encode_key(&kev(PAGEDOWN, false, false, true)), None);
}

#[test]
fn printables_and_line_edits_clear_the_selection() {
    // Printable characters (any case/shift state — shift isn't consulted).
    assert!(clears_selection(&kev("a", false, false, false)));
    assert!(clears_selection(&kev("A", false, false, false)));
    assert!(clears_selection(&kev("5", false, false, false)));
    assert!(clears_selection(&kev(" ", false, false, false)));
    // The line-editing specials.
    assert!(clears_selection(&kev(RETURN, false, false, false)));
    assert!(clears_selection(&kev(BACKSPACE, false, false, false)));
    assert!(clears_selection(&kev(DELETE, false, false, false)));
}

#[test]
fn modifiers_chords_and_navigation_keep_the_selection() {
    // Ctrl-/Alt- combos never clear (Ctrl+C interrupt, app chords, Alt-meta sequences).
    assert!(!clears_selection(&kev("c", true, false, false)));
    assert!(!clears_selection(&kev("v", true, false, false)));
    assert!(!clears_selection(&kev("b", false, true, false)));
    assert!(!clears_selection(&kev(RETURN, true, false, false)));
    // Bare modifier presses (Slint private-use codepoints) never clear.
    assert!(!clears_selection(&kev(CONTROL, false, false, false)));
    assert!(!clears_selection(&kev(SHIFT, false, false, false)));
    assert!(!clears_selection(&kev(ALT, false, false, false)));
    // Navigation / non-editing specials never clear.
    assert!(!clears_selection(&kev(UP, false, false, false)));
    assert!(!clears_selection(&kev(PAGEUP, false, false, false)));
    assert!(!clears_selection(&kev(ESCAPE, false, false, false)));
    assert!(!clears_selection(&kev(TAB, false, false, false)));
    assert!(!clears_selection(&kev(F5, false, false, false)));
    // Empty text (nothing typed) never clears.
    assert!(!clears_selection(&kev("", false, false, false)));
}

#[test]
fn scroll_page_key_classifies_shift_pageup_down_only() {
    assert_eq!(
        scroll_page_key(&kev(PAGEUP, false, false, true)),
        Some(true)
    );
    assert_eq!(
        scroll_page_key(&kev(PAGEDOWN, false, false, true)),
        Some(false)
    );
    // Un-shifted PageUp/Down are NOT scroll keys (they go to the shell).
    assert_eq!(scroll_page_key(&kev(PAGEUP, false, false, false)), None);
    assert_eq!(scroll_page_key(&kev(PAGEDOWN, false, false, false)), None);
    // A plain printable key is never a scroll key.
    assert_eq!(scroll_page_key(&kev("a", false, false, true)), None);
}

#[test]
fn modified_special_keys_carry_an_xterm_modifier() {
    // Ctrl+End → ESC[1;5F (Claude Code's scroll-to-bottom chord; the "Ctrl+End does nothing"
    // fix). Modifier m = 1 + Shift + 2·Alt + 4·Ctrl.
    assert_eq!(
        encode_key(&kev(END, true, false, false)),
        Some(b"\x1b[1;5F".to_vec())
    );
    assert_eq!(
        encode_key(&kev(HOME, true, false, false)),
        Some(b"\x1b[1;5H".to_vec())
    );
    // Ctrl+Right = word-right in many editors → ESC[1;5C.
    assert_eq!(
        encode_key(&kev(RIGHT, true, false, false)),
        Some(b"\x1b[1;5C".to_vec())
    );
    // Alt = 3, Shift = 2; tilde keys take the `n;m~` form.
    assert_eq!(
        encode_key(&kev(DELETE, true, false, false)),
        Some(b"\x1b[3;5~".to_vec())
    );
    assert_eq!(
        encode_key(&kev(UP, false, true, false)),
        Some(b"\x1b[1;3A".to_vec())
    );
    // Unmodified keys keep their plain sequence (no regression).
    assert_eq!(
        encode_key(&kev(END, false, false, false)),
        Some(b"\x1b[F".to_vec())
    );
    assert_eq!(
        encode_key(&kev(UP, false, false, false)),
        Some(b"\x1b[A".to_vec())
    );
}

#[test]
fn scroll_edge_key_classifies_shift_home_end_only() {
    assert_eq!(scroll_edge_key(&kev(HOME, false, false, true)), Some(true)); // → top
    assert_eq!(scroll_edge_key(&kev(END, false, false, true)), Some(false)); // → bottom
                                                                             // Un-shifted Home/End are NOT edge-scroll keys (they encode CSI to the shell).
    assert_eq!(scroll_edge_key(&kev(HOME, false, false, false)), None);
    assert_eq!(scroll_edge_key(&kev(END, false, false, false)), None);
    assert_eq!(scroll_edge_key(&kev("a", false, false, true)), None);
}

#[test]
fn shift_home_end_are_gated_from_the_pty() {
    // The scroll-to-edge gesture must never leak a CSI sequence into the shell.
    assert_eq!(encode_key(&kev(HOME, false, false, true)), None);
    assert_eq!(encode_key(&kev(END, false, false, true)), None);
    // Plain Home/End still reach the shell as their cursor sequences.
    assert_eq!(
        encode_key(&kev(HOME, false, false, false)),
        Some(b"\x1b[H".to_vec())
    );
    assert_eq!(
        encode_key(&kev(END, false, false, false)),
        Some(b"\x1b[F".to_vec())
    );
}

/// R-22: F keys must encode to escape sequences. Before this branch existed they fell
/// through to the printable-text path and emitted the UTF-8 of the X16 sentinel
/// codepoint (F1 → `EF 9C 84`), which silently killed vim's F1 / htop's F2,F5,F9 /
/// less's F1 / tmux's F-key prefixes. The `text` here is the private-use sentinel the
/// app shell must supply (Qt/Slint report empty `text` for special keys), so this also
/// guards the "sentinel leaks into the pty" regression.
#[test]
fn function_keys_encode_to_escape_sequences() {
    // Xterm PC-style function keys have gaps in their CSI numbers.
    assert_eq!(
        encode_key(&kev(F1, false, false, false)),
        Some(b"\x1bOP".to_vec())
    );
    assert_eq!(
        encode_key(&kev(F4, false, false, false)),
        Some(b"\x1bOS".to_vec())
    );
    assert_eq!(
        encode_key(&kev(F5, false, false, false)),
        Some(b"\x1b[15~".to_vec())
    );
    assert_eq!(
        encode_key(&kev(F10, false, false, false)),
        Some(b"\x1b[21~".to_vec())
    );
    // Modified chords ride the same modifier parameter as the other special keys.
    assert_eq!(
        encode_key(&kev(F1, false, false, true)),
        Some(b"\x1b[1;2P".to_vec()),
        "Shift+F1 should carry the xterm modifier parameter"
    );
    assert_eq!(
        encode_key(&kev(F5, true, false, false)),
        Some(b"\x1b[15;5~".to_vec()),
        "Ctrl+F5 should carry the xterm modifier parameter"
    );
    // Whatever the encoding, none of it may be the raw sentinel text.
    for f in [F1, F4, F5, F12] {
        let bytes = encode_key(&kev(f, false, false, false)).unwrap_or_default();
        assert_eq!(bytes.first(), Some(&0x1b), "{f} 首字节不是 ESC");
        assert!(
            String::from_utf8_lossy(&bytes).is_ascii(),
            "{f} 编出了非 ASCII 内容"
        );
    }
}
