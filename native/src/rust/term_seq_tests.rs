//! 维度 2/3/4 的补充无头覆盖（不开 GUI、不碰真服务器）：
//!
//! 补的是 §0.6 之外还空着的三块——**DECSTBM 区域滚动**（vim/less 的分区滚屏）、
//! **被网络分块切断的转义序列**（SSH 每次 read 的边界都可能把 `ESC[38;2;` 切成两半，
//! 引擎必须缓冲而不是把半截画到屏上）、**OSC 8 超链接**，以及**多标签格位互不串台**
//! （维度 4 的第一个可回归断言）。
//!
//! 全部走 TideTerm 自己的注入路径（`term_diag::term_feed_inject` 与运行时
//! `st.pane.feed_bytes` 同一函数），读数只取桥面可观测量：`term_screen_text`（视口文本）
//! 与 `term_stats`（光标格位/网格尺寸）。不喂真 PTY 输出，与机器上装了什么 shell 无关。

use super::term_net_tests::net_serial;
use super::term_tests::input_lock_guard;
use super::{term, term_diag};

// Rendering contract: semantic accents preserve application ANSI colors; only
// populated lines/current cursor get line numbers. Check the actual span bridge.
#[test]
fn semantic_spans_preserve_ansi_and_hide_blank_gutter_rows() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term::term_set_render_opts(tab, 2, true);
    term_diag::term_feed_inject(
        tab,
        b"\x1b[2J\x1b[Hhttps://x.invalid\r\nPS C:\\> git\r\n\x1b[31mC:\\red\x1b[0m",
    );
    let spans = super::term_ops::term_spans(tab);
    let palette = super::term_ops::theme_base16();
    let color = |rgb: [u8; 3]| u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255]);
    let glyph_colors = |ch: char| {
        spans
            .as_chunks::<6>()
            .0
            .iter()
            .filter(|r| r[0] & 255 == 1 && r[4] == ch as u32)
            .map(|r| r[3])
            .collect::<Vec<_>>()
    };
    assert_eq!(glyph_colors('h'), vec![color(palette[14])]);
    assert_eq!(glyph_colors('g'), vec![color(palette[14])]);
    assert_eq!(
        glyph_colors('C'),
        vec![color(palette[11]), color(palette[1])]
    );
    let stamps = super::term_annotations::context_line_times(tab);
    assert_eq!(stamps.len(), 3);
    assert!(stamps
        .iter()
        .all(|s| s.stamp.len() == 10 && s.stamp.starts_with('[')));
    term::term_close_tab(tab);
}

fn rows_of(tab: u32) -> Vec<String> {
    term_diag::term_screen_text(tab)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// DECST-01/DECSTBM（`ESC[t;br`）区域滚动：vim/less 靠它只在窗口分区内滚。
/// 判据取用户视角的一条不变式 —— **区域外的行一个字符都不许动**。
/// 若引擎根本不支持区域滚动，整屏会一起滚，首行/末行立即被冲掉 ⇒ 这条会如实变红。
#[test]
fn decstbm_regional_scroll_leaves_rows_outside_the_region_untouched() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    let rows = term_diag::term_stats(tab).rows as usize;
    assert!(rows >= 8, "标签默认行数太少（{rows}），摆不开区域");

    // 逐行写基线：第 1 行 TOP、中间 MIDn、最后一行 BOTTOM。
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[H");
    term_diag::term_feed_inject(tab, b"TOP");
    for r in 2..rows {
        term_diag::term_feed_inject(tab, format!("\x1b[{r};1HMID{r}").as_bytes());
    }
    term_diag::term_feed_inject(tab, format!("\x1b[{rows};1HBOTTOM").as_bytes());
    let base = rows_of(tab);
    assert_eq!(base[0], "TOP", "基线首行没写进去");
    assert_eq!(base[rows - 1], "BOTTOM", "基线末行没写进去");

    // 设区域 = 第 2 行到倒数第 2 行；**显式 CUP 把光标放到区域底部**（不依赖
    // DECSTBM 之后光标是否归位 —— 那部分各家实现不一致）。
    let (top, bottom) = (2usize, rows - 1);
    term_diag::term_feed_inject(tab, format!("\x1b[{top};{bottom}r").as_bytes());
    term_diag::term_feed_inject(tab, format!("\x1b[{bottom};1H").as_bytes());
    // 往区域里灌「比区域高度更多」的行，逼它滚动。
    for i in 0..rows {
        term_diag::term_feed_inject(tab, format!("SCROLL{i}\r\n").as_bytes());
    }

    let after = rows_of(tab);
    assert_eq!(after.len(), rows, "行数变了：{after:?}");
    assert_eq!(
        after[0], "TOP",
        "区域外的首行被冲掉了 ⇒ DECSTBM 没管住边界：{after:?}"
    );
    assert_eq!(
        after[rows - 1],
        "BOTTOM",
        "区域外的末行被冲掉了 ⇒ DECSTBM 没管住边界：{after:?}"
    );
    // 不变式②：区域**内部**必须真的滚过 —— 原来的 MIDn 全部被顶走、换成 SCROLL 行。
    // （最后一条 `\r\n` 会把区域底部滚成一行空，所以允许一格空。）
    let region = &after[1..rows - 1];
    assert!(
        !region.iter().any(|r| r.starts_with("MID")),
        "区域里还留着基线内容 ⇒ 压根没滚：{after:?}"
    );
    let scrolled = region.iter().filter(|r| r.starts_with("SCROLL")).count();
    assert!(
        scrolled + 2 >= region.len(),
        "区域里 SCROLL 行只有 {scrolled}/{} 行 ⇒ 滚得不够：{after:?}",
        region.len()
    );
    assert!(
        region
            .iter()
            .all(|r| r.is_empty() || r.starts_with("SCROLL")),
        "区域里出现了既不是空行也不是灌进去的内容的行：{after:?}"
    );
    // 收尾：显式复位区域，别把状态漏给同进程里的下一条测例。
    term_diag::term_feed_inject(tab, b"\x1b[r\x1b[2J\x1b[H");
    term::term_close_tab(tab);
}

/// ESC-06：转义序列被**网络分块切断**时必须缓冲，不能把半截当正文画出来。
/// 这不是假想场景：SSH 每次 read 的边界都落在任意字节上，`ESC[38;2;1;2;3m`
/// 完全可能切成三段到达；画出来就是用户看到的 `[38;2;` 噪声。
#[test]
fn escape_sequences_split_across_feeds_are_buffered_not_printed() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[HAB\x1b[3");
    assert_eq!(
        term_diag::term_screen_text(tab),
        "AB",
        "半截序列提前画出来了"
    );

    // 三段才凑齐一个真彩色 SGR：38;2;1;2;3。
    term_diag::term_feed_inject(tab, b"8;2;");
    assert_eq!(
        term_diag::term_screen_text(tab),
        "AB",
        "第二段到达就把中间字符画出来了"
    );
    term_diag::term_feed_inject(tab, b"1;2;3mRED");
    assert_eq!(
        term_diag::term_screen_text(tab),
        "ABRED",
        "三段拼齐后要么丢了序列、要么丢了正文"
    );

    // 裸 ESC 单独成段：下一段的 `[` 必须续上，各自落地会留下可见噪声。
    term_diag::term_feed_inject(tab, b"\x1b");
    term_diag::term_feed_inject(tab, b"[2J");
    assert!(
        term_diag::term_screen_text(tab).is_empty(),
        "裸 ESC 后的续段没接上：{:?}",
        term_diag::term_screen_text(tab)
    );
    term::term_close_tab(tab);
}

/// OSC-8 超链接：URL 本体不许出现在屏上，链接文字要出现。
/// 表现错了就是终端里出现一长串 `http://…` 噪声（ls --hyperlink 之类）。
#[test]
fn osc8_hyperlink_hides_the_url_but_shows_the_anchor_text() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(
        tab,
        b"\x1b[2J\x1b[H\x1b]8;;https://example.com/x\x1b\\LINK\x1b]8;;\x1b\\",
    );
    let text = term_diag::term_screen_text(tab);
    assert!(
        !text.contains("example.com"),
        "URL 本体被画到屏上了：{text:?}"
    );
    assert!(text.contains("LINK"), "链接文字没显示：{text:?}");
    term::term_close_tab(tab);
}

/// 维度 4 的第一条可回归断言：**同一进程里两个标签的格位互不串台**。
/// （交付队列/事件总线是进程全局的，格位同样共享进程 —— 串台的表现为
///  A 屏上出现 B 的文字，或改 A 尺寸把 B 的网格一起改了。）
#[test]
fn two_tabs_keep_their_grids_isolated() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let a = term::term_new_tab();
    let b = term::term_new_tab();
    assert_ne!(a, b, "两个标签拿到同一个 id");

    term_diag::term_feed_inject(a, b"\x1b[2J\x1b[HALPHA_ONLY\r\nA2");
    term_diag::term_feed_inject(b, b"\x1b[2J\x1b[HBETA_ONLY\r\nB2");
    let (ta, tb) = (
        term_diag::term_screen_text(a),
        term_diag::term_screen_text(b),
    );
    assert!(ta.contains("ALPHA_ONLY"), "A 屏上没有自己的内容：{ta:?}");
    assert!(!ta.contains("BETA"), "A 屏上混进了 B 的内容：{ta:?}");
    assert!(tb.contains("BETA_ONLY"), "B 屏上没有自己的内容：{tb:?}");
    assert!(!tb.contains("ALPHA"), "B 屏上混进了 A 的内容：{tb:?}");

    // 尺寸互不牵连：把 A 改小，B 的格数必须原样。
    let before = term_diag::term_stats(b);
    term::term_resize_px(a, 400.0, 300.0);
    let after = term_diag::term_stats(b);
    assert_eq!(
        (after.cols, after.rows),
        (before.cols, before.rows),
        "改 A 的尺寸把 B 的网格一起改了"
    );
    assert!(
        term_diag::term_stats(a).cols < before.cols,
        "A 的缩窗没生效（{} >= {}）",
        term_diag::term_stats(a).cols,
        before.cols
    );

    // 关掉 A 不该影响 B 的读数。
    term::term_close_tab(a);
    assert_eq!(
        term_diag::term_screen_text(b),
        tb,
        "关掉 A 之后 B 的屏幕内容变了"
    );
    term::term_close_tab(b);
}
