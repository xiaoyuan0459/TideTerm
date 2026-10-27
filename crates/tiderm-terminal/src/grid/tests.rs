use super::*;

#[test]
fn feed_advances_grid_and_sets_dirty() {
    let mut g = TermGrid::new(20, 4);
    assert!(g.take_dirty()); // starts dirty
    assert!(!g.take_dirty()); // cleared
    g.feed(b"hi");
    assert!(g.take_dirty());
    let snap = g.snapshot();
    assert_eq!(snap.cell(0, 0).ch, 'h');
    assert_eq!(snap.cell(1, 0).ch, 'i');
}

#[test]
fn resize_reports_change_only_when_size_differs() {
    let mut g = TermGrid::new(20, 4);
    assert!(!g.resize(20, 4));
    assert!(g.resize(40, 10));
    assert_eq!(g.size().cols, 40);
    assert_eq!(g.size().rows, 10);
}

#[test]
fn bel_is_queued_and_drained() {
    let mut g = TermGrid::new(20, 4);
    assert!(!g.take_bell());
    g.feed(b"");
    assert!(g.take_bell(), "BEL byte must surface");
    assert!(!g.take_bell(), "drain consumes it");
}

#[test]
fn osc52_store_queues_clipboard_text() {
    let mut g = TermGrid::new(20, 4);
    // OSC 52 ; c ; base64("hello") with BEL terminator.
    g.feed(b"]52;c;aGVsbG8=");
    let events = g.drain_osc_events();
    assert_eq!(events.len(), 1);
    match &events[0] {
        OscEvent::ClipboardStore(text) => assert_eq!(text, "hello"),
        OscEvent::ClipboardLoad(_) => panic!("expected store, got load"),
    }
    assert!(g.drain_osc_events().is_empty());
}

#[test]
fn osc52_load_queues_formatter_and_replies_via_pty_queue() {
    let mut g = TermGrid::new(20, 4);
    g.feed(b"]52;c;?");
    let events = g.drain_osc_events();
    assert_eq!(events.len(), 1);
    match &events[0] {
        OscEvent::ClipboardStore(_) => panic!("expected load, got store"),
        OscEvent::ClipboardLoad(format) => {
            // the formatter base64-encodes the clipboard text ("abc" → "YWJj")
            assert!(format("abc").contains("YWJj"), "formatter encodes text");
        }
    }
    // The load reply path lands in the pty-reply queue (drained by take_replies).
    g.reply_to_pty(b"reply".to_vec());
    assert_eq!(g.take_replies(), b"reply");
}

#[test]
fn osc_color_query_replies_with_theme_colors() {
    let mut g = TermGrid::new(20, 4);
    // Default: fg(256) = base16[7], bg(257) = base16[0].
    assert_eq!(g.take_replies(), b"");
    g.feed(b"]10;?"); // foreground query
    let fg_reply = g.take_replies();
    assert!(!fg_reply.is_empty(), "OSC 10 query must be answered");
    g.feed(b"]11;?"); // background query
    assert!(
        !g.take_replies().is_empty(),
        "OSC 11 query must be answered"
    );
    // After a theme swap the reply follows the new palette.
    let mut base = [[0u8; 3]; 16];
    base[7] = [0xff, 0x00, 0xff];
    base[0] = [0x12, 0x34, 0x56];
    g.set_base16(base);
    g.feed(b"]10;?");
    let reply = String::from_utf8_lossy(&g.take_replies()).to_string();
    assert!(
        reply.to_lowercase().contains("ffff/0000/ffff"),
        "fg reply = {reply}"
    );
    g.feed(b"]11;?");
    let reply = String::from_utf8_lossy(&g.take_replies()).to_string();
    assert!(
        reply.to_lowercase().contains("1212/3434/5656"),
        "bg reply = {reply}"
    );
}

#[test]
fn osc_title_is_queued_and_collapses_to_latest() {
    let mut g = TermGrid::new(20, 4);
    assert_eq!(g.take_title(), None, "no title until the app sets one");
    // OSC 0;2 (BEL terminator) twice → only the latest survives the drain.
    g.feed(b"\x1b]0;first\x07");
    g.feed(b"\x1b]2;second\x07");
    assert_eq!(g.take_title().as_deref(), Some("second"));
    assert_eq!(g.take_title(), None, "drain consumes the update");

    // Restoring an unset title is an explicit update, distinct from no update.
    // Drain timing must not change the final result of a title push/pop pair.
    for drain_intermediate in [false, true] {
        let mut g = TermGrid::new(20, 4);
        g.feed(b"\x1b[22;0t\x1b]2;temporary\x07");
        if drain_intermediate {
            assert_eq!(g.take_title().as_deref(), Some("temporary"));
        }
        // Split the restore sequence across parser feeds, as a transport may do.
        g.feed(b"\x1b[23;");
        g.feed(b"0t");
        assert_eq!(g.take_title().as_deref(), Some(""), "restore unset title");
        assert_eq!(g.take_title(), None);
        g.feed(b"\x1b]2;base\x07\x1b[22;0t\x1b]2;temporary\x07\x1b[23;0t");
        assert_eq!(g.take_title().as_deref(), Some("base"));
        g.feed(b"\x1b[22;0t\x1b]2;temporary\x07\x1b[23;0t\x1b]2;latest\x07");
        assert_eq!(g.take_title().as_deref(), Some("latest"));
        assert_eq!(g.take_title(), None);
    }
}

/// 2026-09-28 P0 回归：中文行 + 坍缩窗格（请求 1 列）触发 alacritty 缩列 reflow
/// 死循环——宽字符 spacer 分支每轮往 wrapped 头部 insert 回一格，columns==1 时
/// 每轮净减 0，行数无限增长（探针实测 8 GiB 分配 abort 整个进程）。`resize` 把
/// 列数钳到 ≥2（数学上每轮净减 ≥1，循环有界），本测例锁死该形状必须在有界时间
/// 内返回，且缩列前历史已全量排进压缩层（P0b 语义）不丢失。
#[test]
fn cjk_reflow_into_collapsed_pane_terminates() {
    let mut g = TermGrid::new(96, 30);
    for _ in 0..500 {
        g.feed("中文测试 abc 1234\n".as_bytes());
    }
    // 坍缩格请求 1×6 —— 修复前此处无限分配直至 abort。
    g.resize(1, 6);
    assert_eq!(g.size().cols, 2, "1 列请求必须钳到 2 列（reflow 安全下限）");
    assert!(g.total_history() > 0, "缩列前历史应全量排进压缩层");
    // 回到宽格继续喂，网格仍可推进。
    g.resize(96, 30);
    g.feed(b"tail line\n");
    assert!(g.total_history() > 0);
}

#[test]
fn inverse_default_cell_keeps_a_visible_glyph() {
    // ESC[7m = reverse video. Over the default bg, the swap must NOT leave the glyph
    // colour transparent, else reverse-video text vanishes into a solid colour block.
    let mut g = TermGrid::new(20, 4);
    g.feed(b"\x1b[7mX\x1b[0m");
    let snap = g.snapshot();
    let c = snap.cell(0, 0);
    assert_eq!(c.ch, 'X');
    assert_eq!(
        c.fg[3], 0xff,
        "inverse glyph colour must be opaque (visible)"
    );
    assert_eq!(c.bg[3], 0xff, "inverse background must be opaque");
    // The block paints in the default fg; the glyph in the default bg.
    assert_eq!(
        [c.bg[0], c.bg[1], c.bg[2]],
        [snap.default_fg[0], snap.default_fg[1], snap.default_fg[2]]
    );
    assert_eq!(
        [c.fg[0], c.fg[1], c.fg[2]],
        [snap.default_bg[0], snap.default_bg[1], snap.default_bg[2]]
    );
}

#[test]
fn bracketed_paste_mode_tracks_decset_2004() {
    let mut g = TermGrid::new(20, 4);
    assert!(!g.bracketed_paste(), "off by default");
    g.feed(b"\x1b[?2004h"); // DECSET 2004 — app turns bracketed paste ON (PSReadLine does)
    assert!(g.bracketed_paste(), "enabled after DECSET 2004h");
    g.feed(b"\x1b[?2004l"); // DECRST — back off
    assert!(!g.bracketed_paste(), "disabled after DECRST 2004l");
}

// ---- scroll-region correctness + throughput diagnosis (Task 17) ----
//
// The bench flagged native `scrolling-region` at 0.4 MB/s vs `scrolling` at 7.5 (~18x slower)
// and hypothesized an O(n) full-grid memmove per scrolled line in the grid's DECSTBM path.
// That hypothesis does NOT hold for this implementation: the grid is `alacritty_terminal`,
// whose region scroll is an O(1) ring rotate (`Grid::scroll_up`) plus a few O(1) row-pointer
// swaps for the lines fixed outside the region — no whole-grid copy to replace.
//
// The `#[ignore]`d diagnosis below measures it directly. Findings on this machine (4 MiB feed,
// 120x50): feed-only full=49 region=41 MB/s (region ~= full), +snapshot full=25 region=24
// MB/s (parity), +software-render full=1.6 region=2.4 MB/s — region is actually *faster* when
// rendering, because rows below the 20-line region stay blank (fewer glyphs to rasterize).
// So at the terminal-widget level scroll-region is never slower than full-screen scrolling;
// the bench's catastrophe originates ABOVE this crate (session batching / render-pump cadence
// / GPU upload in the app + core), which is outside this track's file scope. No grid fix is
// warranted — these tests instead lock in scroll-within-margins correctness and feed parity.
#[test]
#[ignore]
fn scroll_region_throughput_diagnosis() {
    fn mbps(bytes: usize, d: std::time::Duration) -> f64 {
        (bytes as f64 / (1024.0 * 1024.0)) / d.as_secs_f64()
    }
    // Feed `budget` bytes of `line` in pty-sized chunks after `prologue`; snapshot every
    // `snap_every` lines (0 = never) to mimic the render gate without a font dependency.
    fn run(
        cols: usize,
        rows: usize,
        prologue: &[u8],
        line: &str,
        budget: usize,
        snap_every: usize,
    ) -> std::time::Duration {
        let mut g = TermGrid::new(cols, rows);
        g.feed(prologue);
        let lb = line.as_bytes();
        let mut payload = Vec::with_capacity(budget);
        while payload.len() < budget {
            payload.extend_from_slice(lb);
        }
        let t = std::time::Instant::now();
        let mut since = 0usize;
        for chunk in payload.chunks(4096) {
            g.feed(chunk);
            if snap_every > 0 {
                since += chunk.len() / lb.len().max(1);
                if since >= snap_every {
                    std::hint::black_box(g.snapshot());
                    since = 0;
                }
            }
        }
        t.elapsed()
    }
    let line = "region line 12345 — lorem ipsum dolor sit amet consectetur\r\n";
    let budget = 4 * 1024 * 1024; // 4 MiB, well past the 10k scrollback-growth ramp
    for &(cols, rows) in &[(120usize, 24usize), (120, 50), (200, 80)] {
        let full = run(cols, rows, b"", line, budget, 0);
        let region = run(cols, rows, b"\x1b[1;20r\x1b[H", line, budget, 0);
        let full_s = run(cols, rows, b"", line, budget, 16);
        let region_s = run(cols, rows, b"\x1b[1;20r\x1b[H", line, budget, 16);
        eprintln!(
                "[{cols}x{rows}] feed-only full={:.1} region={:.1} ({:.2}x) | +snapshot full={:.1} region={:.1} ({:.2}x)  MB/s",
                mbps(budget, full), mbps(budget, region), full.as_secs_f64() / region.as_secs_f64(),
                mbps(budget, full_s), mbps(budget, region_s), full_s.as_secs_f64() / region_s.as_secs_f64(),
            );
    }
}

/// First char of each viewport row (blanks as space) — a compact way to assert scroll layout.
fn col0_chars(g: &TermGrid) -> Vec<char> {
    let snap = g.snapshot();
    (0..snap.rows).map(|r| snap.cell(0, r).ch).collect()
}

#[test]
fn scroll_within_midscreen_region_keeps_lines_outside_it() {
    // DECSTBM region NOT anchored at the top exercises alacritty's swap-based region rotate.
    // Region = 1-based rows 2..=4 (0-based 1,2,3). Rows 0 and 4,5 are fixed.
    let mut g = TermGrid::new(8, 6);
    // Seed each row with a distinct first char via absolute cursor moves.
    g.feed(b"\x1b[1;1Ha\x1b[2;1Hb\x1b[3;1Hc\x1b[4;1Hd\x1b[5;1He\x1b[6;1Hf");
    assert_eq!(col0_chars(&g), vec!['a', 'b', 'c', 'd', 'e', 'f']);
    g.feed(b"\x1b[2;4r"); // set scroll region rows 2..=4
    g.feed(b"\x1b[4;1H"); // park the cursor on the region's bottom line (1-based row 4)
    g.feed(b"\n"); // line-feed there scrolls the region up by one
                   // Region rows scroll (b drops out, blank appears at the bottom of the region); the lines
                   // ABOVE (a) and BELOW (e, f) the region must be untouched.
    assert_eq!(col0_chars(&g), vec!['a', 'c', 'd', ' ', 'e', 'f']);
}

#[test]
fn scroll_within_top_anchored_region_keeps_lines_below_it() {
    // The bench's case: region anchored at the top (1-based 1..=3 → 0-based 0,1,2), which
    // takes alacritty's ring-rotate + fixed-bottom swap path. Rows 3,4,5 stay put; the
    // scrolled-out top line goes to scrollback.
    let mut g = TermGrid::new(8, 6);
    g.feed(b"\x1b[1;1Ha\x1b[2;1Hb\x1b[3;1Hc\x1b[4;1Hd\x1b[5;1He\x1b[6;1Hf");
    g.feed(b"\x1b[1;3r"); // region rows 1..=3
    g.feed(b"\x1b[3;1H"); // bottom line of the region (1-based row 3)
    g.feed(b"\n");
    assert_eq!(col0_chars(&g), vec!['b', 'c', ' ', 'd', 'e', 'f']);
    // The scrolled-out 'a' went into scrollback (history grew, viewport still pinned).
    assert!(g.display_offset() == 0);
}

// ---- SGR attribute → RenderCell style mapping (feedback #7: PSReadLine predictions) ----

#[test]
fn sgr_bold_and_italic_set_style_flags() {
    let mut g = TermGrid::new(20, 4);
    g.feed(b"\x1b[1mB\x1b[0m\x1b[3mI\x1b[0m\x1b[1;3mZ\x1b[0m");
    let snap = g.snapshot();
    assert!(snap.cell(0, 0).bold && !snap.cell(0, 0).italic);
    assert!(snap.cell(1, 0).italic && !snap.cell(1, 0).bold);
    assert!(
        snap.cell(2, 0).bold && snap.cell(2, 0).italic,
        "SGR 1;3 sets both"
    );
}

#[test]
fn sgr_dim_blends_fg_toward_background() {
    // SGR 2 over the (transparent) default bg: the glyph colour must move toward the
    // default background — strictly darker than normal text on this dark theme, but
    // not equal to the bg (still legible).
    let mut g = TermGrid::new(20, 4);
    g.feed(b"N \x1b[2mD\x1b[0m");
    let snap = g.snapshot();
    let normal = snap.cell(0, 0).fg;
    let dim = snap.cell(2, 0).fg;
    assert_eq!(snap.cell(2, 0).ch, 'D');
    assert_ne!(dim, normal, "dim must change the drawn colour");
    for i in 0..3 {
        assert!(
            dim[i] < normal[i] && dim[i] > snap.default_bg[i],
            "dim channel {i} must sit between bg and fg (bg={} dim={} fg={})",
            snap.default_bg[i],
            dim[i],
            normal[i]
        );
    }
    // Exactly the documented 45/55 blend.
    let expect = dim_blend(normal, snap.default_bg);
    assert_eq!(dim, expect);
}

#[test]
fn sgr_dim_blends_toward_explicit_cell_bg() {
    // Dim over an explicit (non-default) background blends toward THAT bg, not the
    // pane default. SGR 44 = blue bg, SGR 2 = dim.
    let mut g = TermGrid::new(20, 4);
    g.feed(b"\x1b[44;2mX\x1b[0m");
    let snap = g.snapshot();
    let c = snap.cell(0, 0);
    assert!(c.bg[3] > 0, "explicit bg must be opaque");
    assert_eq!(c.fg, dim_blend(snap.default_fg, c.bg));
}

#[test]
fn sgr_dim_and_bold_combine() {
    // SGR 1;2 — alacritty tracks DIM and BOLD independently; both must survive into
    // the cell (bold face, dimmed colour).
    let mut g = TermGrid::new(20, 4);
    g.feed(b"\x1b[1;2mY\x1b[0m");
    let snap = g.snapshot();
    let c = snap.cell(0, 0);
    assert!(c.bold, "bold flag survives alongside dim");
    assert_eq!(c.fg, dim_blend(snap.default_fg, snap.default_bg));
}

#[test]
fn indexed_256_and_truecolor_foregrounds_resolve() {
    // PSReadLine's default prediction colour is an indexed dim gray (e.g. 238);
    // truecolor must pass through verbatim.
    let mut g = TermGrid::new(20, 4);
    g.feed(b"\x1b[38;5;238mA\x1b[0m\x1b[38;2;10;200;30mB\x1b[0m");
    let snap = g.snapshot();
    // Index 238 is on the grayscale ramp: 8 + (238-232)*10 = 68.
    assert_eq!(snap.cell(0, 0).fg, [68, 68, 68, 0xff]);
    assert_eq!(snap.cell(1, 0).fg, [10, 200, 30, 0xff]);
}

#[test]
fn psreadline_prediction_style_dim_italic_256gray() {
    // The exact shape PSReadLine emits for inline predictions: faint + italic + a dim
    // 256-colour gray, ended with SGR 0.
    let mut g = TermGrid::new(40, 4);
    g.feed(b"\x1b[2;3;38;5;238mGet-ChildItem\x1b[0m");
    let snap = g.snapshot();
    let c = snap.cell(0, 0);
    assert_eq!(c.ch, 'G');
    assert!(c.italic, "prediction renders italic");
    assert!(!c.bold);
    let gray = [68u8, 68, 68, 0xff]; // index 238
    assert_eq!(
        c.fg,
        dim_blend(gray, snap.default_bg),
        "dim applied on top of the 256-colour gray"
    );
}

#[test]
fn dsr_query_produces_a_reply_to_forward() {
    let mut g = TermGrid::new(20, 4);
    // ESC[6n — Device Status Report (cursor position). The terminal must answer or a
    // real conpty blocks; we forward the queued reply to the pty.
    g.reply_to_pty(b"prefix".to_vec());
    g.feed(b"a\x1b[6n\x1b[5nb\x1b[6n");
    assert_eq!(g.take_replies(), b"prefix\x1b[1;2R\x1b[0n\x1b[1;3R");
    assert!(g.take_replies().is_empty(), "replies are consumed once");
}
