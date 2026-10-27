//! Initial terminal modes; subsequent application escape sequences remain authoritative.
use tiderm_core::session_options::SessionOptions;
use tiderm_terminal::TerminalPane;

pub(super) fn apply(pane: &mut TerminalPane, options: &SessionOptions) {
    for (enabled, set, reset) in [
        (
            options.auto_wrap,
            b"\x1b[?7h".as_slice(),
            b"\x1b[?7l".as_slice(),
        ),
        (
            options.newline_mode,
            b"\x1b[20h".as_slice(),
            b"\x1b[20l".as_slice(),
        ),
        (
            options.app_cursor,
            b"\x1b[?1h".as_slice(),
            b"\x1b[?1l".as_slice(),
        ),
        (options.app_keypad, b"\x1b=".as_slice(), b"\x1b>".as_slice()),
    ] {
        pane.feed_bytes(if enabled { set } else { reset });
    }
}
