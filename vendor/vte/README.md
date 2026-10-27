vte
===

[![Build Status](https://travis-ci.org/alacritty/vte.svg?branch=master)](https://travis-ci.org/alacritty/vte)
[![Crates.io Version](https://img.shields.io/crates/v/vte.svg)](https://crates.io/crates/vte/)

Parser for implementing virtual terminal emulators in Rust.

The parser is implemented according to [Paul Williams' ANSI parser state
machine]. The state machine doesn't assign meaning to the parsed data and is
thus not itself sufficient for writing a terminal emulator. Instead, it is
expected that an implementation of the `Perform` trait which does something
useful with the parsed data. The `Parser` handles the book keeping, and the
`Perform` gets to simply handle actions.

See the [docs] for more info.

[Paul Williams' ANSI parser state machine]: https://vt100.net/emu/dec_ansi_parser
[docs]: https://docs.rs/crate/vte/

TideTerm local changes
----------------------

The synchronized-update buffer is allocated lazily. The ANSI `Handler` also has
an optional, default-empty `shell_marker(u8)` callback for OSC 133 A/B/C/D. This
lets retained command/input anchors observe the parser's actual execution order,
including synchronized replay, without a second byte parser in the UI host.
Other handlers can retain their existing behavior without implementing it.
