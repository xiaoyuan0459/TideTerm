//! Typed Qt neutral key codes. No synthetic toolkit text is needed.
use tiderm_terminal::keys::Key;

pub(crate) fn key_from_code(code: u32) -> Key {
    match code {
        1 => Key::Up,
        2 => Key::Down,
        3 => Key::Left,
        4 => Key::Right,
        5 => Key::Home,
        6 => Key::End,
        7 => Key::PageUp,
        8 => Key::PageDown,
        9 => Key::Insert,
        10 => Key::Delete,
        11 => Key::Backspace,
        12 => Key::Tab,
        13 => Key::Backtab,
        14 => Key::Return,
        15 => Key::Escape,
        c if (32..44).contains(&c) => Key::F((c - 32) as u8 + 1),
        _ => Key::Unhandled,
    }
}
