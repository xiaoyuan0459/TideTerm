//! P1 memory-budget harness — a *reproducible* byte-count report for the steady-state RAM
//! of one terminal pane under the P1 caps:
//!   * GPU atlas 2048² → 1024² (1 MB/pane, gpu feature only)
//!   * font glyph cache + resolve memo: LRU-capped (was unbounded)
//!   * scrollback store 64 MB → 32 MB default (configurable via `with_history`)
//!   * bg instance buffer initial cap 4096 → 1024 (gpu feature only)
//!
//! Run (software build — the app's default):
//!   cargo test --test memory_budget_p1 -- --nocapture
//! Run (with the GPU stack, to also report atlas/bg caps as live allocations):
//!   cargo test --features gpu --test memory_budget_p1 -- --nocapture
//!
//! The report prints per-pane upper bounds and asserts the live caches never exceed their
//! caps even under a long, mixed (blank + text + CJK) session.

use tiderm_terminal::font::GlyphKey;
use tiderm_terminal::{scrollback::ScrollbackStore, Font, TermGrid};

const MB: f64 = 1024.0 * 1024.0;
const KB: f64 = 1024.0;

/// Feed `n` representative lines into `g`: ~60% are near-blank (prompt + spaces; the common
/// case in real output — interning makes them ~free) and ~40% are wide varied text, so the
/// compressed store exercises RLE + dedup rather than only blanks.
fn feed_mixed(g: &mut TermGrid, n: usize, cols: usize) {
    let mut s = String::with_capacity(n * (cols + 2));
    for i in 0..n {
        if i % 5 == 0 {
            s.push_str(&format!("$\x20{}\r\n", " ".repeat(cols.saturating_sub(2))));
        } else {
            s.push_str(&format!(
                "L{i:06}\x20{}\r\n",
                "x".repeat(cols.saturating_sub(8))
            ));
        }
    }
    g.feed(s.as_bytes());
}

#[test]
fn scrollback_stays_within_p1_budget() {
    // Lock the P1 decision in code: the default compressed cap is 32 MB.
    assert_eq!(
        tiderm_terminal::grid::DEFAULT_COMPRESSED_CAP,
        32 * 1024 * 1024,
        "P1 scrollback default must be 32 MB"
    );

    // A large, realistic pane (200×50) on a long session.
    let mut g = TermGrid::new(200, 50);
    feed_mixed(&mut g, 120_000, 200);

    let bytes = g.scrollback_bytes();
    let budget = g.scrollback_budget();
    println!(
        "[scrollback] pane 200x50 after 120k mixed lines: {:.2} MB retained / {:.2} MB budget ({} lines kept)",
        bytes as f64 / MB,
        budget as f64 / MB,
        g.total_history(),
    );
    assert!(
        bytes <= budget,
        "scrollback store exceeded its byte budget: {} > {}",
        bytes,
        budget
    );
}

#[test]
fn font_glyph_cache_is_bounded_by_lru() {
    // Headless Font from the bundled JetBrains Mono (no filesystem access).
    let bytes = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
    let mut font = Font::from_bytes(bytes, 14.0).expect("bundled font must load");

    // ---- Realistic sample: glyphs the font actually covers ----
    // ASCII + Latin-1 Supplement + Box Drawing + Block Elements + a slice of CJK.
    let mut realistic = 0usize;
    for cp in (0x20u32..0x7f)
        .chain(0xA0..0x100)
        .chain(0x2500..0x2580)
        .chain(0x4e00..0x4e00 + 256)
    {
        if let Some(ch) = char::from_u32(cp) {
            let (fid, gid) = font.resolve(ch);
            if gid != 0 {
                font.rasterize(GlyphKey {
                    font_id: fid,
                    gid,
                    bold: false,
                    italic: false,
                });
                realistic += 1;
            }
        }
    }
    let real_cache = font.cache_len();
    let real_bytes = font.cache_bytes();
    println!(
        "[font] realistic: {} covered glyphs -> cache {} entries ({:.2} KB mask bytes), resolve {} entries",
        realistic,
        real_cache,
        real_bytes as f64 / KB,
        font.resolve_cache_len(),
    );
    assert!(
        real_cache <= 8192,
        "glyph cache exceeded GLYPH_CACHE_CAP: {}",
        real_cache
    );

    // ---- Stress sweep: force far more distinct keys than the cap (0..9000 synthetic gids)
    // so the LRU *must* evict. Proves the cap holds for CJK-heavy / pathological sessions. ----
    for gid in 0..9000u16 {
        font.rasterize(GlyphKey {
            font_id: 0,
            gid,
            bold: false,
            italic: false,
        });
    }
    let stress_cache = font.cache_len();
    println!(
        "[font] after 9000-key stress sweep: cache {} entries (<= 8192 cap enforced by LRU)",
        stress_cache,
    );
    assert!(
        stress_cache <= 8192,
        "LRU failed to cap the glyph cache: {} > 8192",
        stress_cache
    );
    assert!(
        font.resolve_cache_len() <= 4096,
        "resolve cache exceeded RESOLVE_CACHE_CAP: {}",
        font.resolve_cache_len()
    );
}

#[test]
fn scrollback_store_drops_oldest_within_budget() {
    // Direct store-level invariant: a store built with the P1 default never exceeds it.
    let mut s = ScrollbackStore::new(32 * 1024 * 1024);
    // ~80k unique 200-col text lines (~856 B each) would be ~68 MB raw → must be trimmed to 32 MB.
    for i in 0..80_000usize {
        let line: String = format!("row{i:05} {}", "y".repeat(190));
        let chars: Vec<u16> = line.chars().map(|c| c as u16).collect();
        s.push(tiderm_terminal::scrollback::LineData {
            chars: chars.into(),
            extras: Vec::new(),
            runs: vec![tiderm_terminal::scrollback::StyleRun {
                start: 0,
                len: 200,
                fg: 0,
                bg: 0,
                bold: false,
                italic: false,
                underline: false,
                wide: false,
            }],
            cols: 200,
            wrap: false,
        });
    }
    println!(
        "[store] 80k unique 200-col lines -> {:.2} MB retained / 32 MB budget ({} rows kept)",
        s.estimated_bytes() as f64 / MB,
        s.rows(),
    );
    assert!(
        s.estimated_bytes() <= 32 * 1024 * 1024,
        "store exceeded budget: {}",
        s.estimated_bytes()
    );
}

#[test]
fn ring_grows_beyond_64k_when_budget_allows() {
    // P1.1: the ring must grow past 2^16 rows when the byte budget has headroom, so
    // short-line sessions (logs, build output) keep far more than 65k lines per pane.
    // 90k unique ~10-col lines ≈ 90k × 86 B ≈ 7.7 MB — well under the 1 GB budget, so
    // only the ring size can be the limiter; growth must kick in instead of overwriting.
    let mut s = ScrollbackStore::new(1 << 30);
    for i in 0..90_000usize {
        let line: String = format!("row{i:07}");
        let chars: Vec<u16> = line.chars().map(|c| c as u16).collect();
        let ncols = chars.len() as u16;
        s.push(tiderm_terminal::scrollback::LineData {
            chars: chars.into(),
            extras: Vec::new(),
            runs: vec![tiderm_terminal::scrollback::StyleRun {
                start: 0,
                len: ncols,
                fg: 0,
                bg: 0,
                bold: false,
                italic: false,
                underline: false,
                wide: false,
            }],
            cols: 200,
            wrap: false,
        });
    }
    println!(
        "[ring] 90k unique 10-col lines -> {} rows kept (grew past 2^16 = {} rows)",
        s.rows(),
        1 << 16,
    );
    assert!(
        s.rows() > (1 << 16),
        "ring must grow past 2^16 when budget allows: {}",
        s.rows()
    );
    assert_eq!(
        s.rows(),
        90_000,
        "no rows may be dropped under a huge budget"
    );
    // Oldest retained row is the very first pushed — growth re-lays rows in order.
    let first = s.row(0).expect("row 0");
    assert_eq!(
        first
            .chars
            .iter()
            .map(|&u| char::from_u32(u as u32).unwrap())
            .collect::<String>(),
        "row0000000"
    );
}

#[test]
fn report_per_pane_upper_bound() {
    // Aggregate per-pane steady-state upper bound under P1 caps.
    let scrollback = 32.0 * MB;
    let glyph_ceil = 8192.0 * 384.0; // cap * generous max mask (14px cell masks are ~16×24 ≈ 384 B)
    let resolve_ceil = 4096.0 * 16.0;

    println!("\n=== P1 per-pane steady-state RAM upper bound (single pane) ===");
    println!(
        "  scrollback store (compressed)      : {:>8.2} MB (ring grows 2^16..2^20 rows)",
        scrollback / MB
    );
    println!(
        "  font glyph masks (LRU cap 8192)    : {:>8.2} MB (theoretical ceiling)",
        glyph_ceil / MB
    );
    println!(
        "  font resolve memo (LRU cap 4096)   : {:>8.2} KB",
        resolve_ceil / KB
    );

    #[cfg(feature = "gpu")]
    {
        let atlas = tiderm_terminal::render::atlas_bytes();
        let bg = tiderm_terminal::render::bg_buffer_bytes();
        println!(
            "  GPU atlas texture (1024² R8)        : {:>8.2} MB",
            atlas as f64 / MB
        );
        println!(
            "  GPU bg instance buffer (1024)      : {:>8.2} KB",
            bg as f64 / KB
        );
        println!("  (gpu feature ON — these ARE allocated per pane)");
    }
    #[cfg(not(feature = "gpu"))]
    {
        println!("  GPU atlas texture (1024² R8)        :    1.00 MB (compile-time cap; gpu OFF -> not allocated)");
        println!("  GPU bg instance buffer (1024)      :   ~0.09 MB (compile-time cap; gpu OFF -> not allocated)");
        println!("  (gpu feature OFF — the app's default software build does NOT allocate these)");
    }

    println!("\n  --- vs pre-P1 baseline (per pane, x N panes/tabs) ---");
    println!("  scrollback : 64 MB -> 32 MB   (-32 MB)");
    println!("  atlas      :  4 MB ->  1 MB   (-3 MB x panes, gpu only)");
    println!("  glyph cache: unbounded -> <=~3 MB (LRU)");
    println!("  bg_cap     : 4096 -> 1024      (~-96 KB initial, gpu only)");
}
