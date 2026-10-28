//! Keyboard encoding and input routing for actual terminal sessions.
use super::term::with_tab;
use super::term_keys::key_from_code;
use tiderm_terminal::keys::{
    encode_key, scroll_edge_key, scroll_page_key, Key, KeyEvent, Modifiers,
};

/// 中立键码（R5 桥面约定）：0=Char 走 text；1..15 功能键；32+n = F(n+1)；其余
/// Unhandled（编码器按 text 透传 —— 与 Slint 壳逐字节同语义）。
pub fn term_key(idx: u32, key: u32, mods: u32, text: &str) -> Vec<u8> {
    if super::term_ops::input_locked() {
        return Vec::new();
    }
    let bytes = with_tab(idx, |st| {
        if super::term_reading::local_key(st, key, mods, text) {
            return Vec::new();
        }
        if st.host.modem.busy() {
            return Vec::new();
        }
        let ev = KeyEvent {
            key: if key == 0 {
                Key::classify(text)
            } else {
                key_from_code(key)
            },
            text,
            mods: Modifiers {
                ctrl: mods & 4 != 0,
                alt: mods & 2 != 0,
                shift: mods & 1 != 0,
                meta: false,
            },
        };
        // 引擎约定（keys.rs:252-255「the app shell calls this first」）：
        // Shift+PgUp/PgDn 与 Shift+Home/End 是**回滚手势**，滚动视口且**不写 pty**。
        // 桥面此前只在 term_scroll_page/edge 暴露动作、两侧都无人调用 ⇒ 键盘回滚不可达。
        if let Some(up) = scroll_page_key(&ev) {
            st.pane.scroll_page(up);
            return Vec::new();
        }
        if let Some(top) = scroll_edge_key(&ev) {
            if top {
                st.pane.scroll_to_top();
            } else {
                st.pane.scroll_to_bottom();
            }
            return Vec::new();
        }
        let keypad = if mods & 8 != 0 && mods & 6 == 0 && st.pane.app_keypad() {
            match text {
                "0" => Some(b'p'),
                "1" => Some(b'q'),
                "2" => Some(b'r'),
                "3" => Some(b's'),
                "4" => Some(b't'),
                "5" => Some(b'u'),
                "6" => Some(b'v'),
                "7" => Some(b'w'),
                "8" => Some(b'x'),
                "9" => Some(b'y'),
                "." => Some(b'n'),
                "," => Some(b'l'),
                "+" => Some(b'k'),
                "-" => Some(b'm'),
                "*" => Some(b'j'),
                "/" => Some(b'o'),
                _ if key == 14 => Some(b'M'),
                _ => None,
            }
        } else {
            None
        };
        let bytes = if let Some(code) = keypad {
            vec![27, b'O', code]
        } else {
            match (key, mods & 7) {
                (1..=6, 0) if st.pane.app_cursor() => {
                    vec![
                        27,
                        b'O',
                        match key {
                            1 => b'A',
                            2 => b'B',
                            3 => b'D',
                            4 => b'C',
                            5 => b'H',
                            _ => b'F',
                        },
                    ]
                }
                (11, 0) => vec![if st.config.meta.options.backspace_del {
                    127
                } else {
                    8
                }],
                (10, 0) => match st.config.meta.options.delete_mode {
                    1 => vec![127],
                    2 => vec![8],
                    _ => b"\x1b[3~".to_vec(),
                },
                (14, 0) => match st.config.meta.options.enter_mode {
                    1 => vec![10],
                    2 => vec![13, 10],
                    _ => vec![13],
                },
                _ => encode_key(&ev).unwrap_or_default(),
            }
        };
        if !bytes.is_empty() {
            super::terminal_completion::record_edit(st, key, mods, text);
            super::command_history::submitted(st, &bytes);
            st.cursor.reset();
            st.host.write(&bytes);
        }
        bytes
    });
    super::term_ops::broadcast_others(idx, &bytes);
    bytes
}

pub fn term_write(idx: u32, bytes: &[u8]) {
    if super::term_ops::input_locked() {
        return;
    }
    if super::term::try_with_tab(idx, |st| st.reading || st.host.modem.busy()).unwrap_or(true) {
        return;
    }
    with_tab(idx, |st| {
        super::terminal_completion::record_edit(
            st,
            0,
            0,
            std::str::from_utf8(bytes).unwrap_or("\0"),
        );
        super::command_history::submitted(st, bytes);
        st.cursor.reset();
        st.host.write(bytes);
    });
    super::term_ops::broadcast_others(idx, bytes);
}
