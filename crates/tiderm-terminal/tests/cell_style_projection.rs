//! VT styles have the same observable appearance before and after compression.
use tiderm_terminal::{GridSnapshot, TermGrid};

struct Case {
    name: &'static str,
    sgr: &'static str,
    fg: [u8; 4],
    bg: [u8; 4],
    attrs: [bool; 3],
    glyph: char,
    wide: bool,
}

fn grid() -> TermGrid {
    let mut grid = TermGrid::with_history(12, 3, 2, 1 << 20);
    let mut theme = [[0; 3]; 16];
    theme[0] = [10, 20, 30];
    theme[7] = [210, 220, 230];
    grid.set_base16(theme);
    grid
}

fn check(case: &Case, phase: &str, snapshot: &GridSnapshot, failures: &mut Vec<String>) {
    let cell = snapshot.cell(0, 0);
    if cell.ch != case.glyph
        || cell.fg != case.fg
        || cell.bg != case.bg
        || [cell.bold, cell.italic, cell.underline] != case.attrs
        || cell.wide != case.wide
        || cell.wide_spacer
    {
        failures.push(format!(
            "{}/{phase}: glyph={:?} fg={:?} bg={:?} attributes={:?} wide={}",
            case.name,
            cell.ch,
            cell.fg,
            cell.bg,
            [cell.bold, cell.italic, cell.underline],
            cell.wide,
        ));
    }
    let reset = snapshot.cell(if case.wide { 2 } else { 1 }, 0);
    if reset.ch != 'Y'
        || reset.fg != [210, 220, 230, 255]
        || reset.bg != [0, 0, 0, 0]
        || reset.bold
        || reset.italic
        || reset.underline
    {
        failures.push(format!(
            "{}/{phase}: SGR reset did not restore plain Y",
            case.name
        ));
    }
    if case.wide && (!snapshot.cell(1, 0).wide_spacer || snapshot.cell(1, 0).bg != case.bg) {
        failures.push(format!(
            "{}/{phase}: wide spacer background changed",
            case.name
        ));
    }
}

#[test]
fn sgr_styles_survive_compression_and_reset_boundaries() {
    let cases = [
        Case {
            name: "inverse",
            sgr: "7",
            fg: [10, 20, 30, 255],
            bg: [210, 220, 230, 255],
            attrs: [false; 3],
            glyph: 'X',
            wide: false,
        },
        Case {
            name: "dim",
            sgr: "2",
            fg: [100, 110, 120, 255],
            bg: [0, 0, 0, 0],
            attrs: [false; 3],
            glyph: 'X',
            wide: false,
        },
        Case {
            name: "dim on explicit background",
            sgr: "38;2;200;100;50;48;2;20;40;60;2",
            fg: [101, 67, 55, 255],
            bg: [20, 40, 60, 255],
            attrs: [false; 3],
            glyph: 'X',
            wide: false,
        },
        Case {
            name: "inverse then dim",
            sgr: "38;2;200;100;50;48;2;20;40;60;7;2",
            fg: [119, 73, 54, 255],
            bg: [200, 100, 50, 255],
            attrs: [false; 3],
            glyph: 'X',
            wide: false,
        },
        Case {
            name: "indexed attributes",
            sgr: "1;3;4;38;5;238",
            fg: [68, 68, 68, 255],
            bg: [0, 0, 0, 0],
            attrs: [true; 3],
            glyph: 'X',
            wide: false,
        },
        Case {
            name: "wide inverse dim attributes",
            sgr: "38;2;200;100;50;48;2;20;40;60;1;3;4;7;2",
            fg: [119, 73, 54, 255],
            bg: [200, 100, 50, 255],
            attrs: [true; 3],
            glyph: '中',
            wide: true,
        },
        Case {
            name: "truecolor",
            sgr: "38;2;12;34;56;48;2;78;90;123",
            fg: [12, 34, 56, 255],
            bg: [78, 90, 123, 255],
            attrs: [false; 3],
            glyph: 'X',
            wide: false,
        },
    ];
    let mut failures = Vec::new();
    for case in cases {
        let mut grid = grid();
        grid.feed(format!("\x1b[{}m{}\x1b[0mY", case.sgr, case.glyph).as_bytes());
        check(&case, "live", &grid.snapshot(), &mut failures);
        grid.feed(b"\r\n");
        for _ in 0..20 {
            grid.feed(b"next\r\n");
        }
        assert!(grid.total_history() > 2, "must exercise compressed history");
        grid.scroll_to_top();
        check(&case, "stored", &grid.snapshot(), &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn blank_rows_keep_their_background_after_compression() {
    for (sgr, expected) in [
        ("48;2;78;90;123", [78, 90, 123, 255]),
        ("7", [210, 220, 230, 255]),
    ] {
        let mut grid = grid();
        grid.feed(format!("\x1b[{sgr}m            \x1b[0m").as_bytes());
        let live = grid.snapshot();
        assert_eq!(live.cell(0, 0).bg, expected);
        assert_eq!(live.cell(11, 0).bg, expected);
        grid.feed(b"\r\n");
        for _ in 0..20 {
            grid.feed(b"next\r\n");
        }
        assert!(grid.total_history() > 2);
        grid.scroll_to_top();
        let stored = grid.snapshot();
        assert_eq!(stored.cell(0, 0).ch, ' ');
        assert_eq!(
            stored.cell(0, 0).bg,
            expected,
            "SGR {sgr} blank background disappeared"
        );
        assert_eq!(stored.cell(11, 0).bg, expected);
    }
}
