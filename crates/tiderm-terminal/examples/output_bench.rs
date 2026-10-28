//! Release-only engine benchmark: parser/history, live snapshots and Qt span records.
//! Run from the repository root with `cargo run --release --locked --manifest-path
//! crates/tiderm-terminal/Cargo.toml --example output_bench`. No PTY, font rasterizer,
//! Qt event loop or network participates. One warmup and three samples per workload.

use std::{fmt::Write, hint::black_box, time::Instant};
use tiderm_terminal::{spans, GridSnapshot, RenderOpts, SoftwareRenderer, TermGrid, TerminalPane};

const COLS: usize = 200;
const ROWS: usize = 50;
const LINES: usize = 100_000;
const FRAMES: usize = 10_000;
const SPAN_FRAMES: usize = 2_000;

fn payload(styled: bool) -> Vec<u8> {
    let mut text = String::with_capacity(LINES * 240);
    for line in 0..LINES {
        write!(text, "{line:08} ").expect("write to String");
        for part in 0..4 {
            if styled {
                text.push_str(["\x1b[31m", "\x1b[1;32m", "\x1b[2;34m", "\x1b[0m"][part]);
            }
            for col in 0..47 {
                text.push(char::from(b'a' + ((line + col + part) % 26) as u8));
            }
        }
        text.push_str("\x1b[0m\r\n");
    }
    text.into_bytes()
}

fn records(snapshot: &GridSnapshot) -> Vec<u32> {
    let metrics = spans::Metrics {
        cell_w: 10,
        cell_h: 20,
        ascent: 15,
    };
    let opts = RenderOpts {
        cursor_on: true,
        cursor_style: 0,
        gutter: false,
    };
    spans::collect(snapshot, &metrics, &opts)
}

fn fingerprint(snapshot: &GridSnapshot) -> u64 {
    records(snapshot)
        .into_iter()
        .fold(0xcbf29ce484222325, |hash, word| {
            (hash ^ u64::from(word)).wrapping_mul(0x100000001b3)
        })
}

fn feed(bytes: &[u8]) -> (f64, u64, usize) {
    let mut pane = TerminalPane::new(COLS, ROWS, Box::new(SoftwareRenderer::new()));
    let start = Instant::now();
    for chunk in bytes.chunks(16 * 1024) {
        pane.feed_bytes(black_box(chunk));
    }
    // Include all background compression in the timed work, not just queue admission.
    let retained = pane.total_history();
    let elapsed = start.elapsed().as_secs_f64();
    // Read the real retained content after timing, preventing an unused-output benchmark.
    let signature = pane.screen_text().bytes().fold(0_u64, |sum, b| {
        sum.wrapping_mul(31).wrapping_add(u64::from(b))
    });
    (elapsed, signature, retained)
}

fn snapshots<const DRAW: bool>(grid: &TermGrid, frames: usize) -> (f64, u64, usize) {
    let mut cells = Vec::new();
    let start = Instant::now();
    for _ in 0..frames {
        let mut snapshot = grid.snapshot_into(&mut cells);
        black_box(&snapshot);
        if DRAW {
            black_box(records(&snapshot));
        }
        cells = std::mem::take(&mut snapshot.cells);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let snapshot = grid.snapshot_into(&mut cells);
    (elapsed, fingerprint(&snapshot), snapshot.cells.len())
}

fn samples(name: &str, units: usize, mut run: impl FnMut() -> (f64, u64, usize)) {
    let mut seconds = [0_f64; 3];
    for index in 0..4 {
        let (elapsed, signature, retained) = run();
        assert!(elapsed.is_finite() && elapsed > 0.0, "invalid measurement");
        let kind = if index == 0 { "warmup" } else { "sample" };
        println!("workload={name} kind={kind} index={index} seconds={elapsed:.6} units_per_second={:.2} signature={signature:016x} retained={retained}", units as f64 / elapsed);
        if index > 0 {
            seconds[index - 1] = elapsed;
        }
    }
    seconds.sort_by(f64::total_cmp);
    println!(
        "workload={name} median_seconds={:.6} median_units_per_second={:.2}",
        seconds[1],
        units as f64 / seconds[1]
    );
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("run with --release");
        std::process::exit(2);
    }
    println!("profile=release backend=engine grid={COLS}x{ROWS} lines={LINES} frames={FRAMES} span_frames={SPAN_FRAMES} chunk_bytes=16384 warmup=1 samples=3");
    let plain = payload(false);
    let styled = payload(true);
    samples("feed_plain", LINES, || feed(&plain));
    samples("feed_styled", LINES, || feed(&styled));

    let mut grid = TermGrid::new(COLS, ROWS);
    grid.feed(&styled);
    samples("snapshot_live", FRAMES, || {
        snapshots::<false>(&grid, FRAMES)
    });
    samples("spans_live", SPAN_FRAMES, || {
        snapshots::<true>(&grid, SPAN_FRAMES)
    });
    grid.set_view_offset(1000);
    samples("snapshot_history", FRAMES, || {
        snapshots::<false>(&grid, FRAMES)
    });
    samples("spans_history", SPAN_FRAMES, || {
        snapshots::<true>(&grid, SPAN_FRAMES)
    });
}
