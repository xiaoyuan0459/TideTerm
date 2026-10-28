//! TideTerm input encoding. Functional keys do not depend on a toolkit text field.
//! VT contracts: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Backspace,
    Tab,
    Backtab,
    Return,
    Escape,
    F(u8),
    Char(char),
    Unhandled,
}

impl Key {
    pub fn classify(text: &str) -> Self {
        match text.chars().next() {
            Some('\x08') => Self::Backspace,
            Some('\t') => Self::Tab,
            Some('\n') => Self::Return,
            Some('\x1b') => Self::Escape,
            Some('\x19') => Self::Backtab,
            Some('\x7f') => Self::Delete,
            Some('\u{f700}') => Self::Up,
            Some('\u{f701}') => Self::Down,
            Some('\u{f702}') => Self::Left,
            Some('\u{f703}') => Self::Right,
            Some(c @ '\u{f704}'..='\u{f70f}') => Self::F((c as u32 - 0xf703) as u8),
            Some('\u{f727}') => Self::Insert,
            Some('\u{f729}') => Self::Home,
            Some('\u{f72b}') => Self::End,
            Some('\u{f72c}') => Self::PageUp,
            Some('\u{f72d}') => Self::PageDown,
            Some(c)
                if (0x20..0x7f).contains(&(c as u32))
                    || (0x80..0xe000).contains(&(c as u32))
                    || c as u32 > 0xf8ff =>
            {
                Self::Char(c)
            }
            _ => Self::Unhandled,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent<'a> {
    pub key: Key,
    pub text: &'a str,
    pub mods: Modifiers,
}

impl<'a> KeyEvent<'a> {
    /// Preserve the neutral text-event API for non-Qt consumers. Qt supplies typed
    /// functional keys directly and never needs synthetic text through this path.
    pub fn from_text(text: &'a str, ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
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
}

fn final_sequence(final_byte: u8, modifier: u8, ss3: bool) -> Vec<u8> {
    if modifier == 1 {
        vec![0x1b, if ss3 { b'O' } else { b'[' }, final_byte]
    } else {
        format!("\x1b[1;{modifier}{}", char::from(final_byte)).into_bytes()
    }
}
fn tilde_sequence(number: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{number}~").into_bytes()
    } else {
        format!("\x1b[{number};{modifier}~").into_bytes()
    }
}

pub fn encode_key(event: &KeyEvent<'_>) -> Option<Vec<u8>> {
    if scroll_page_key(event).is_some() || scroll_edge_key(event).is_some() {
        return None;
    }
    let modifier = 1
        + u8::from(event.mods.shift)
        + 2 * u8::from(event.mods.alt)
        + 4 * u8::from(event.mods.ctrl);
    let bytes = match event.key {
        Key::Up => final_sequence(b'A', modifier, false),
        Key::Down => final_sequence(b'B', modifier, false),
        Key::Right => final_sequence(b'C', modifier, false),
        Key::Left => final_sequence(b'D', modifier, false),
        Key::Home => final_sequence(b'H', modifier, false),
        Key::End => final_sequence(b'F', modifier, false),
        Key::PageUp => tilde_sequence(5, modifier),
        Key::PageDown => tilde_sequence(6, modifier),
        Key::Insert => tilde_sequence(2, modifier),
        Key::Delete => tilde_sequence(3, modifier),
        Key::F(n @ 1..=4) => final_sequence(b'P' + n - 1, modifier, true),
        Key::F(n @ 5..=12) => tilde_sequence(
            [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)],
            modifier,
        ),
        Key::F(_) => return None,
        Key::Return => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Backtab => b"\x1b[Z".to_vec(),
        Key::Tab if event.mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::Char(_) | Key::Unhandled => {
            if event.text.is_empty() {
                return None;
            }
            if event.mods.ctrl {
                let control = match event.text.chars().next()? {
                    c @ ('a'..='z' | 'A'..='Z') => Some(c.to_ascii_uppercase() as u8 - b'A' + 1),
                    ' ' | '@' => Some(0),
                    '[' => Some(0x1b),
                    '\\' => Some(0x1c),
                    ']' => Some(0x1d),
                    '^' => Some(0x1e),
                    '_' => Some(0x1f),
                    _ => None,
                };
                if let Some(control) = control {
                    return Some(vec![control]);
                }
            }
            let mut text = Vec::with_capacity(event.text.len() + usize::from(event.mods.alt));
            if event.mods.alt {
                text.push(0x1b);
            }
            text.extend_from_slice(event.text.as_bytes());
            text
        }
    };
    Some(bytes)
}

pub fn scroll_page_key(event: &KeyEvent<'_>) -> Option<bool> {
    match (event.mods.shift, event.key) {
        (true, Key::PageUp) => Some(true),
        (true, Key::PageDown) => Some(false),
        _ => None,
    }
}
pub fn scroll_edge_key(event: &KeyEvent<'_>) -> Option<bool> {
    match (event.mods.shift, event.key) {
        (true, Key::Home) => Some(true),
        (true, Key::End) => Some(false),
        _ => None,
    }
}
pub fn is_printable(event: &KeyEvent<'_>) -> bool {
    !event.mods.ctrl && !event.mods.alt && matches!(event.key, Key::Char(_))
}
pub fn clears_selection(event: &KeyEvent<'_>) -> bool {
    !event.mods.ctrl
        && !event.mods.alt
        && (matches!(event.key, Key::Return | Key::Backspace | Key::Delete) || is_printable(event))
}

#[cfg(test)]
mod tests;
