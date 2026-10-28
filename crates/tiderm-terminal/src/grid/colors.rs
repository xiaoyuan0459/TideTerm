//! Grid color and row conversion primitives.
use super::*;

#[inline]
pub(super) fn dim_blend(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    const KEEP_NUM: u32 = 45; // keep 45% of fg → 55% toward bg
    const DEN: u32 = 100;
    let mix =
        |f: u8, b: u8| -> u8 { ((f as u32 * KEEP_NUM + b as u32 * (DEN - KEEP_NUM)) / DEN) as u8 };
    [
        mix(fg[0], bg[0]),
        mix(fg[1], bg[1]),
        mix(fg[2], bg[2]),
        0xff,
    ]
}

/// Expand a compressed [`LineData`] row into `cols` [`RenderCell`]s. A `wide` run (len 2)
/// paints the glyph on its first column and a spacer on its second.
pub(super) fn render_cells_from_line_data(ld: &LineData, cols: usize) -> Vec<RenderCell> {
    let mut out = vec![RenderCell::default(); cols];
    for run in &ld.runs {
        for i in 0..run.len {
            let col = run.start as usize + i as usize;
            if col >= cols {
                break;
            }
            let ch = ld
                .extras
                .iter()
                .find(|(c, _)| *c as usize == col)
                .and_then(|(_, text)| text.chars().next())
                .or_else(|| {
                    ld.chars
                        .get(col)
                        .copied()
                        .and_then(|u| char::from_u32(u as u32))
                })
                .unwrap_or(' ');
            out[col] = RenderCell {
                ch,
                fg: unpack_rgba(run.fg),
                bg: unpack_rgba(run.bg),
                bold: run.bold,
                italic: run.italic,
                underline: run.underline,
                wide: run.wide && i == 0,
                wide_spacer: run.wide && i == 1,
            };
        }
    }
    out
}

/// Pack an RGBA byte array into a `u32` (little-endian byte order: r, g, b, a).
#[inline]
pub(super) fn pack_rgba(c: [u8; 4]) -> u32 {
    u32::from_le_bytes(c)
}

/// Unpack a `u32` RGBA (little-endian byte order) back into `[r, g, b, a]`.
#[inline]
pub(super) fn unpack_rgba(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// Tokyo-Night-ish default 16 + the standard xterm 256-colour cube/grayscale.
pub(super) fn default_palette() -> [Rgb; 256] {
    let mut p = [Rgb { r: 0, g: 0, b: 0 }; 256];
    let base: [[u8; 3]; 16] = [
        [0x16, 0x16, 0x1e], // 0 black (used as default bg)
        [0xf7, 0x76, 0x8e], // 1 red
        [0x9e, 0xce, 0x6a], // 2 green
        [0xe0, 0xaf, 0x68], // 3 yellow
        [0x7a, 0xa2, 0xf7], // 4 blue
        [0xbb, 0x9a, 0xf7], // 5 magenta
        [0x7d, 0xcf, 0xff], // 6 cyan
        [0xc0, 0xca, 0xf5], // 7 white (used as default fg)
        [0x41, 0x48, 0x68], // 8 bright black
        [0xf7, 0x76, 0x8e], // 9 bright red
        [0x9e, 0xce, 0x6a], // 10 bright green
        [0xe0, 0xaf, 0x68], // 11 bright yellow
        [0x7a, 0xa2, 0xf7], // 12 bright blue
        [0xbb, 0x9a, 0xf7], // 13 bright magenta
        [0x7d, 0xcf, 0xff], // 14 bright cyan
        [0xff, 0xff, 0xff], // 15 bright white
    ];
    for (i, c) in base.iter().enumerate() {
        p[i] = Rgb {
            r: c[0],
            g: c[1],
            b: c[2],
        };
    }
    // 6x6x6 colour cube (indices 16..=231).
    let levels = [0u8, 95, 135, 175, 215, 255];
    let mut idx = 16;
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                p[idx] = Rgb {
                    r: levels[r],
                    g: levels[g],
                    b: levels[b],
                };
                idx += 1;
            }
        }
    }
    // Grayscale ramp (indices 232..=255).
    for i in 0..24 {
        let v = 8 + i as u8 * 10;
        p[232 + i] = Rgb { r: v, g: v, b: v };
    }
    p
}
