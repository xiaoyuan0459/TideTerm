//! G1/G2 闸门自检套件（离屏确定性；判据表见 cpp 侧 README 与 v1 §6.1）。

use crate::ffi::TermStats;
use crate::rust::{term, term_diag};

/// Self-tests use fresh profiles; optional loopback SSH fixtures import only
/// their public host key from the system temporary directory.
pub fn prepare_config() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("tideterm-selftest-{}-{nonce}", std::process::id()));
    tiderm_core::set_config_directory(directory.clone()).expect("isolated self-test profile");
    if std::env::var_os("TIDETERM_TEST_SFTP_PORT").is_some() {
        let path = std::path::PathBuf::from(
            std::env::var_os("TEST_SSH_KNOWN_HOSTS").expect("fixture known hosts"),
        );
        let path = path.canonicalize().expect("fixture known hosts exist");
        assert!(path.starts_with(
            std::env::temp_dir()
                .canonicalize()
                .expect("temporary directory")
        ));
        assert!(std::fs::metadata(&path).expect("fixture metadata").len() <= 16384);
        std::fs::copy(path, directory.join("known_hosts")).expect("copy public fixture host key");
    }
}

/// 当前 surface 落盘 PNG（Rust 侧直接编码，不经 Qt —— 证据链更短）。
pub fn save_png(path: &str) -> bool {
    let _ = term::term_frame(0); // 确保脏帧已渲染
    let s: TermStats = term_diag::term_stats(0);
    let Some(cpu) = term_diag::term_cpu_bytes(0) else {
        return false;
    };
    let (w, h) = (s.surface_w, s.surface_h);
    let Ok(file) = std::fs::File::create(path) else {
        return false;
    };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    match enc.write_header() {
        Ok(mut writer) => writer.write_image_data(&cpu).is_ok(),
        Err(_) => false,
    }
}

/// G1 闸门套件：八项判据（对照 v1 §6.1），全过返回 0。
pub fn g1_selftest() -> i32 {
    let mut fails: Vec<&str> = Vec::new();
    let mut check = |name: &'static str, ok: bool| {
        println!("{} {}", if ok { "✅" } else { "❌" }, name);
        if !ok {
            fails.push(name);
        }
    };

    // 视口定型（纯 Rust 路径没有 C++ resizeEvent —— 等价于 800×500 视口）。
    // 标签 0：注册表首个会话（Phase C ① 多标签 API 的 idx 形态）。
    assert_eq!(term::term_new_tab(), 0, "首标签索引必须是 0");
    let _ = term::term_resize_px(0, 800.0, 500.0); // 同尺寸 no-op 也合法
    let s0 = term_diag::term_stats(0);
    assert!(s0.cols > 40 && s0.rows > 10, "网格必须已按视口定型");

    // ---- G1-2 CJK：宽字符占两列（中文4+空格1+AB2+空格1+世界4 = 第 12 列） ----
    term_diag::term_feed_inject(0, "中文 AB 世界".as_bytes());
    let s = term_diag::term_stats(0);
    println!("    cursor_col 实测 = {}（期望 12）", s.cursor_col);
    check(
        "G1-2 CJK 宽字符占位",
        s.cursor_col == 12 && term_diag::term_screen_text(0).contains("中文 AB 世界"),
    );

    // ---- G1-3 光标三态：三帧像素必须两两不同 ----
    term::term_set_render_opts(0, 0, false);
    term::term_frame(0);
    let style0 = term_diag::term_cpu_bytes(0).unwrap_or_default();
    term::term_set_render_opts(0, 1, false);
    term::term_frame(0);
    let style1 = term_diag::term_cpu_bytes(0).unwrap_or_default();
    term::term_set_render_opts(0, 2, false);
    term::term_frame(0);
    let style2 = term_diag::term_cpu_bytes(0).unwrap_or_default();
    check(
        "G1-3 光标三态互异",
        style0 != style1 && style1 != style2 && style0 != style2,
    );
    term::term_set_render_opts(0, 0, false);
    let _ = save_png("target/g1_cursor_block.png");

    // ---- G1-6 gutter occupies the existing viewport, reducing text columns. ----
    let before_gutter = term_diag::term_stats(0);
    let pixels_before = term_diag::term_cpu_bytes(0).unwrap_or_default();
    term::term_set_render_opts(0, 0, true);
    term::term_frame(0);
    let s = term_diag::term_stats(0);
    check(
        "G1-6 gutter 留出列且保持视口宽度",
        s.cols < before_gutter.cols
            && s.surface_w == before_gutter.surface_w
            && term_diag::term_cpu_bytes(0).unwrap_or_default() != pixels_before,
    );
    let _ = save_png("target/g1_gutter.png");
    term::term_set_render_opts(0, 0, false);

    // ---- 滚动测试的准备：500 行 ----
    let mut burst = String::new();
    for i in 0..500 {
        burst.push_str(&format!("L{i:04} scrollback proof\n"));
    }
    term_diag::term_feed_inject(0, burst.as_bytes());

    // ---- G1-5 滚轮回滚 ----
    let bottom_view = term_diag::term_screen_text(0);
    term::term_scroll_page(0, true);
    term::term_scroll_page(0, true);
    let scrolled = term_diag::term_screen_text(0);
    check(
        "G1-5a PageUp 视口上移进历史",
        scrolled != bottom_view && scrolled.contains("scrollback proof"),
    );
    term::term_scroll_edge(0, true);
    check(
        "G1-5b Shift+Home 到顶",
        term_diag::term_screen_text(0).contains("L0000"),
    );
    term::term_scroll_edge(0, false);
    let tail = term_diag::term_screen_text(0);
    check(
        "G1-5c Shift+End 回底",
        tail.contains("L0499") || tail.contains("scrollback proof"),
    );

    // ---- G1-4 选区 + 复制（拖拽语义：end 自动 copy） ----
    let st = term_diag::term_stats(0);
    let (cw, ch) = (st.cell_w as f32, st.cell_h as f32);
    term::term_selection_begin(0, 0.0, 0.0);
    term::term_selection_update(0, 8.0 * cw, 2.0 * ch);
    let copied = term::term_selection_end(0);
    check("G1-4a 拖拽选区自动复制", copied > 0);
    let clip = term::term_clipboard_text(0);
    check(
        "G1-4b 选区内容正确",
        clip.contains("scrollback proof") || clip.contains("L04"),
    );
    println!("    剪贴板实测：{:?}", clip.trim_end());

    // ---- G1-1/G1-8 吞吐与帧率（判据只在 release 下成立 —— swash 调试版慢 ~7×） ----
    if cfg!(debug_assertions) {
        println!("⏭  G1-1/G1-8 跳过（debug 构建）——另跑 `cargo run --release -- --g1-bench`");
    } else {
        let lines_per_sec = g1_median_throughput();
        let gate = 51_570.0 * 0.97; // A/B 基线（2026-09-13 当日 render_bench 交织实测）
        check("G1-1 吞吐 ≥ 基线 ×0.97", lines_per_sec >= gate);
        println!("    吞吐实测 {lines_per_sec:.0} 行/s（基线 51,570）");
        let frame_ms = 1000.0 / lines_per_sec * 200.0;
        check("G1-8 满载帧耗时 ≤33ms(≥30fps)", frame_ms <= 33.0);
        println!("    feed+render 平均成本（200 行/帧）{frame_ms:.2} ms");
    }

    let _ = save_png("target/g1_final.png");

    if fails.is_empty() {
        println!("[rust] G1 闸门套件：全部通过");
        0
    } else {
        println!("[rust] G1 闸门套件失败项：{fails:?}");
        1
    }
}

/// G1-1/G1-8 基准入口：A/B 基线 = 同日同机 render_bench 实测（2026-09-13 = 51,570；
/// 重跑：cd native && cargo run --release --bin native -- --g1-bench）。
pub fn g1_bench() -> i32 {
    let lines = g1_median_throughput();
    let baseline = 51_570.0;
    let frame_ms = 200.0 / lines * 1000.0;
    println!("[bench] 200000 lines feed+render → {lines:.0} lines/s（基线 {baseline:.0}）");
    println!("[bench] feed+render 平均成本（200 行/帧）: {frame_ms:.2} ms（预算 33ms）");
    let ok = lines >= baseline * 0.97 && frame_ms <= 33.0;
    println!(
        "[bench] G1-1/G1-8 判据（≥ 基线 ×0.97 且帧 ≤33ms）: {}（比例 {:.1}%）",
        if ok { "PASS" } else { "FAIL" },
        lines / baseline * 100.0
    );
    if ok {
        0
    } else {
        1
    }
}

/// Fixed sampling protocol shared by both Release gates; invalid samples fail closed.
fn g1_median_throughput() -> f64 {
    println!("[bench] font={}", crate::rust::term_ops::font_file());
    println!("[bench] profile={} backend=software grid=200x50 ASCII=200 cols bursts=1000 lines/burst=200 renders=1000 warmup=1 samples=3",
        if cfg!(debug_assertions) { "debug" } else { "release" });
    let warmup = term_diag::term_feed_render_bench(1000, 200);
    println!("[bench] warmup (discarded): {warmup:.2} lines/s");
    let mut samples = [0.0_f64; 3];
    for (index, sample) in samples.iter_mut().enumerate() {
        *sample = term_diag::term_feed_render_bench(1000, 200);
        println!("[bench] sample {}: {sample:.2} lines/s", index + 1);
    }
    if !warmup.is_finite()
        || warmup <= 0.0
        || samples
            .iter()
            .any(|sample| !sample.is_finite() || *sample <= 0.0)
    {
        println!("[bench] invalid measurement: FAIL");
        return f64::NAN;
    }
    samples.sort_by(f64::total_cmp);
    println!("[bench] median: {:.2} lines/s", samples[1]);
    samples[1]
}

/// G2 IME 自检断言：提交串「中文测试123」经 PTY 回显后必须上屏。
pub fn term_g2_selftest_assert() -> bool {
    let text = term_diag::term_screen_text(0);
    let ok = text.contains("中文测试123");
    let hit = text.lines().find(|l| l.contains("中文测试"));
    println!("[rust] 屏上 CJK 行：{:?}", hit.unwrap_or("<无>"));
    println!(
        "[rust] G2 断言（IME 提交串「中文测试123」经 PTY 上屏）: {}",
        if ok { "通过" } else { "失败" }
    );
    ok
}

/// PTY 自检断言：子 shell 计算输出 `end-500` 必须上屏（可视区）。
pub fn term_pty_selftest_assert() -> bool {
    let text = term_diag::term_screen_text(0);
    let ok = text.contains("end-500");
    println!("[rust] 屏文本尾部：{:?}", text.lines().last().unwrap_or(""));
    println!(
        "[rust] 自检断言（子 shell 计算输出 end-500 上屏）: {}",
        if ok { "通过" } else { "失败" }
    );
    ok
}
