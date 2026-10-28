//! Headless proof for the line-number gutter (`RenderOpts.gutter`): renders the same
//! 40-line transcript twice — gutter off (left) vs gutter on (right) — with the viewport
//! scrolled up into history, composed into one side-by-side PNG. Asserts:
//!   * the gutter strip contains non-background pixels (numbers were drawn);
//!   * scrolling shifts `row_numbers` (top row number decreases with `view_offset`).
//!
//! Run: `cargo run --example line_gutter_check` → writes `target/line_gutter_check.png`.

use tiderm_terminal::{PaneRenderer, RenderOpts, SoftwareRenderer, TermGrid};

const GAP: u32 = 12;

fn make_grid() -> TermGrid {
    let mut grid = TermGrid::new(40, 8);
    for i in 1..=40 {
        grid.feed(format!("line {i:02} — gutter proof\r\n").as_bytes());
    }
    grid
}

fn main() -> anyhow::Result<()> {
    let font_path = if std::path::Path::new("C:/Windows/Fonts/CascadiaMono.ttf").exists() {
        "C:/Windows/Fonts/CascadiaMono.ttf"
    } else {
        "C:/Windows/Fonts/consola.ttf"
    };
    let px = 28.0_f32;
    let mut font = tiderm_terminal::Font::from_path(font_path, px)?;

    // Scroll up into history so the gutter shows non-live rows.
    let mut grid = make_grid();
    let (history, _rows, _off) = grid.scroll_metrics();
    let target = history.saturating_sub(4);
    grid.set_view_offset(target);
    let (buffer_lines, row_numbers) = (grid.buffer_lines(), {
        let s = grid.snapshot();
        s.row_numbers.clone()
    });
    assert_eq!(row_numbers.len(), 8);
    assert_eq!(
        row_numbers[7],
        buffer_lines - target as u64,
        "bottom row must show buffer_lines − view_offset"
    );
    assert_eq!(
        row_numbers[0],
        buffer_lines - target as u64 - 7,
        "row numbers must decrease toward the top"
    );
    eprintln!(
        "buffer={buffer_lines} view_offset={target} top={} bottom={}",
        row_numbers[0], row_numbers[7]
    );

    // Render gutter-off vs gutter-on at the same scroll position.
    let mut r = SoftwareRenderer::new();
    let (off_img, w0, h) = {
        let snap = grid.snapshot();
        let sfc = r.render(
            &snap,
            &mut font,
            &RenderOpts {
                cursor_on: true,
                cursor_style: 0,
                gutter: false,
            },
        );
        let cpu = sfc.as_cpu().expect("cpu surface");
        (cpu.as_rgba_bytes().to_vec(), cpu.width(), cpu.height())
    };
    let (on_img, w, h2) = {
        let snap = grid.snapshot();
        let sfc = r.render(
            &snap,
            &mut font,
            &RenderOpts {
                cursor_on: true,
                cursor_style: 0,
                gutter: true,
            },
        );
        let cpu = sfc.as_cpu().expect("cpu surface");
        (cpu.as_rgba_bytes().to_vec(), cpu.width(), cpu.height())
    };
    assert_eq!(h2, h, "heights must match");
    assert!(w > w0, "gutter render must be wider by the strip");
    // Gutter strip (left `w − w0` columns) must contain ink: the numbers were drawn.
    let strip = w - w0;
    let mut ink = 0u32;
    for y in 0..h {
        for x in 0..strip {
            let o = ((y * w + x) * 4) as usize;
            if on_img[o] != 0x16 || on_img[o + 1] != 0x16 || on_img[o + 2] != 0x1e {
                ink += 1;
            }
        }
    }
    assert!(ink > 0, "gutter strip must contain drawn number pixels");
    eprintln!("gutter strip {strip}px wide, {ink} ink pixels");

    // Compose side by side.
    let ow = w0 + w + GAP;
    let mut out = vec![0u8; (ow * h * 4) as usize];
    for p in out.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[0x16, 0x16, 0x1e, 0xff]);
    }
    for y in 0..h {
        let d0 = ((y * ow) * 4) as usize;
        let s0 = ((y * w0) * 4) as usize;
        out[d0..d0 + (w0 * 4) as usize].copy_from_slice(&off_img[s0..s0 + (w0 * 4) as usize]);
        let d1 = ((y * ow + w0 + GAP) * 4) as usize;
        let s1 = ((y * w) * 4) as usize;
        out[d1..d1 + (w * 4) as usize].copy_from_slice(&on_img[s1..s1 + (w * 4) as usize]);
    }

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/line_gutter_check.png");
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap())?;
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), ow, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&out)?;
    eprintln!("wrote {path}  (left=off · right=on, scrolled into history)");
    Ok(())
}
