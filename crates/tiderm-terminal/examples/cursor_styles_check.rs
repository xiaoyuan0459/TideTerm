//! Headless proof for the three cursor styles (`RenderOpts.cursor_style` = 0/1/2):
//! renders the same line with the cursor parked over a printed glyph once per style,
//! composed top-to-bottom into one PNG:
//!   * row 1 — **block**: full-cell true-invert (glyph stays readable over the block);
//!   * row 2 — **underline**: thin fg band hugging the cell bottom, glyph untouched;
//!   * row 3 — **bar**: thin fg band hugging the cell left edge, glyph untouched.
//!
//! Run: `cargo run --example cursor_styles_check` → writes `target/cursor_styles_check.png`.

use tiderm_terminal::{PaneRenderer, RenderOpts, SoftwareRenderer, TermGrid};

const GAP: u32 = 10;

fn render_style(
    r: &mut SoftwareRenderer,
    grid: &TermGrid,
    font: &mut tiderm_terminal::Font,
    style: u8,
) -> (Vec<u8>, u32, u32) {
    let sfc = r.render(
        &grid.snapshot(),
        font,
        &RenderOpts {
            cursor_on: true,
            cursor_style: style,
            gutter: false,
        },
    );
    let cpu = sfc
        .as_cpu()
        .expect("software renderer must yield a cpu surface");
    (cpu.as_rgba_bytes().to_vec(), cpu.width(), cpu.height())
}

fn main() -> anyhow::Result<()> {
    let font_path = if std::path::Path::new("C:/Windows/Fonts/CascadiaMono.ttf").exists() {
        "C:/Windows/Fonts/CascadiaMono.ttf"
    } else {
        "C:/Windows/Fonts/consola.ttf"
    };
    let px = 28.0_f32;
    let mut font = tiderm_terminal::Font::from_path(font_path, px)?;
    let (cw, ch) = (font.cell_w, font.cell_h);

    // One row: "style_check AB" with the cursor parked over the 'A' (CHA column 14).
    let mut grid = TermGrid::new(24, 1);
    grid.feed(b"style_check AB\x1b[14G");
    let snap = grid.snapshot();
    let (ccol, crow) = snap.cursor;
    eprintln!(
        "cursor over '{}' at cell ({ccol},{crow}); cell {cw}x{ch}px",
        snap.cell(ccol, crow).ch
    );

    // Render the three styles; assert each output differs from the block baseline.
    let mut frames = Vec::new();
    let mut renderer = SoftwareRenderer::new();
    for style in [0u8, 1, 2] {
        let (buf, w, h) = render_style(&mut renderer, &grid, &mut font, style);
        frames.push((buf, w, h));
    }
    let (base, w, h) = (&frames[0], frames[0].1, frames[0].2);
    for (i, (buf, _, _)) in frames.iter().enumerate().skip(1) {
        let diff = buf
            .iter()
            .zip(base.0.iter())
            .filter(|(a, b)| a != b)
            .count();
        eprintln!("style {i} vs block: {diff} differing bytes");
        assert!(diff > 0, "style {i} must differ from the block cursor");
    }

    // Compose the three frames into one PNG (top → bottom: block, underline, bar).
    let oh = h * 3 + GAP * 2;
    let mut out = vec![0u8; (w * oh * 4) as usize];
    for p in out.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[0x16, 0x16, 0x1e, 0xff]);
    }
    for (i, (buf, fw, fh)) in frames.iter().enumerate() {
        let y_off = i as u32 * (h + GAP);
        debug_assert_eq!((*fw, *fh), (w, h));
        for y in 0..*fh {
            let dst = (((y + y_off) * w) * 4) as usize;
            let s = ((y * w) * 4) as usize;
            out[dst..dst + (w * 4) as usize].copy_from_slice(&buf[s..s + (w * 4) as usize]);
        }
    }

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/target/cursor_styles_check.png"
    );
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap())?;
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, oh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&out)?;
    eprintln!("wrote {path}  (top=block · middle=underline · bottom=bar)");
    Ok(())
}
