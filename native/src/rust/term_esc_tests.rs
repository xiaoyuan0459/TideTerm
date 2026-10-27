//! 维度 2/3 的无头桥面测例：转义序列（光标定位 / 擦除 / 备用屏 / SGR / 进度条 /
//! 双宽字形）与窗口自适应重排。与 `term_tests.rs` 同族，拆成单独文件只为躲 500 行门禁。
//!
//! 这些测例走 TideTerm **自己的**注入路径（`term_feed_inject` 与运行时 `term.rs`
//! 的 `st.pane.feed_bytes` 同一函数），而不是直接调 vendored 引擎——引擎层的
//! autowrap / ED / DECSTBM / alt-screen / reflow 已由 `grid_reference.rs` 自证，
//! 壳层真正没被覆盖的是「字节有没有原样送到引擎」与「桥面读数有没有被改写」。
//! 故断言只取桥面可观测量：`term_screen_text`（视口文本）+ `term_stats`（光标格位、
//! 网格尺寸）。不喂真 PTY 输出，因此与机器上装了什么 shell 无关，可重复。

use super::{term, term_diag, term_tests::input_lock_guard};

/// 视口文本按行切开（`screen_text` 已裁行尾空白与尾部空行）。
fn rows_of(tab: u32) -> Vec<String> {
    term_diag::term_screen_text(tab)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// 只留非空白字符：软换行会让同一段文本占多行，比对总内容时行数是噪声。
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 以「格数」请求 resize，返回**实际**生效尺寸。桥面收逻辑像素、`cells_for_px`
/// 做整数除法，故 `格数 × 单元格边长` 落在同一个商里。断言用返回值而非请求值，
/// HiDPI（SCALE≠1）下不会假失败，但尺寸没变会如实暴露。
fn resize_cells(tab: u32, cols: usize, rows: usize) -> (u32, u32) {
    let st = term_diag::term_stats(tab);
    term::term_resize_px(
        tab,
        cols as f32 * st.cell_w as f32,
        rows as f32 * st.cell_h as f32,
    );
    let after = term_diag::term_stats(tab);
    (after.cols, after.rows)
}

/// 两条各 1.5×列宽的软换行逻辑行（无空格 → 比对总文本时零噪声），连 TAIL 的全文。
fn wrapped_pair(cols: usize) -> (String, String, String) {
    let cyc = |start: u8, n: usize| -> String {
        (0..n).map(|i| (start + (i % 26) as u8) as char).collect()
    };
    let l1 = cyc(b'a', cols + cols / 2);
    let l2 = cyc(b'A', cols + cols / 2);
    let expect = format!("{l1}{l2}TAIL");
    (l1, l2, expect)
}

/// ESC-01 光标定位与擦除：CUP/CUU/CUB/EL/ED 经桥面生效。
/// vim/top 的重绘全靠这组序列，任一被吞的表现就是屏幕留残影。
#[test]
fn cursor_addressing_and_erase_sequences_take_effect() {
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[HAAA\r\nBBB\r\nCCC\x1b[5;7HX");
    let rows = rows_of(tab);
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(rows[0], "AAA");
    assert_eq!(rows[2], "CCC");
    assert_eq!(rows[3], "", "第 4 行应是空行");
    assert_eq!(rows[4], "      X", "CUP(5;7) 后的写入没落在第 5 行第 7 列");
    let st = term_diag::term_stats(tab);
    assert_eq!(
        (st.cursor_col, st.cursor_row),
        (7, 4),
        "写 X 后光标应停在第 5 行第 8 格（0-based）"
    );

    // 相对移动：上 2 行到第 3 行、左 3 格（CUU/CUB）。
    term_diag::term_feed_inject(tab, b"\x1b[2A\x1b[3D");
    let st = term_diag::term_stats(tab);
    assert_eq!((st.cursor_col, st.cursor_row), (4, 2), "CUU/CUB 位移不对");

    // EL：只擦光标所在行，别行的内容必须原样留着。
    term_diag::term_feed_inject(tab, b"\x1b[2K");
    let rows = rows_of(tab);
    assert_eq!(rows[2], "", "ESC[2K 未擦除当前行");
    assert_eq!(rows[0], "AAA", "ESC[2K 波及了别的行");
    assert_eq!(rows[4], "      X", "ESC[2K 波及了别的行");
    // ED：整屏清空。
    term_diag::term_feed_inject(tab, b"\x1b[2J");
    assert!(
        term_diag::term_screen_text(tab).is_empty(),
        "ESC[2J 未清屏：{:?}",
        term_diag::term_screen_text(tab)
    );
    term::term_close_tab(tab);
}

/// ESC-02 备用屏（DECSET 1049h/l）：vim、nano、htop、less 的全屏切换。
/// 判据取用户最在意的两点——进 alt 屏不污染主屏、退出后主屏原样回来。
#[test]
fn alternate_screen_swaps_viewport_and_restores_primary() {
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[HPRIMARY_ONE\r\nPRIMARY_TWO\x1b[5;3H");
    let saved = term_diag::term_stats(tab);
    assert_eq!((saved.cursor_col, saved.cursor_row), (2, 4), "基线光标");

    term_diag::term_feed_inject(tab, b"\x1b[?1049h");
    let s = term_diag::term_screen_text(tab);
    assert!(!s.contains("PRIMARY_ONE"), "进 alt 屏仍显示主屏内容：{s}");

    term_diag::term_feed_inject(tab, b"ALT_FRAME");
    term_diag::term_feed_inject(tab, b"\x1b[?1049l");
    let s = term_diag::term_screen_text(tab);
    assert!(
        s.contains("PRIMARY_ONE") && s.contains("PRIMARY_TWO"),
        "退 alt 屏主屏没回来：{s}"
    );
    assert!(!s.contains("ALT_FRAME"), "alt 屏内容渗进主屏：{s}");
    let st = term_diag::term_stats(tab);
    assert_eq!(
        (st.cursor_col, st.cursor_row),
        (saved.cursor_col, saved.cursor_row),
        "1049l 未恢复进屏前的光标（vim 退出后提示符会错位）"
    );
    term::term_close_tab(tab);
}

/// ESC-03 SGR（16 色 / 256 色 / 真彩）与私有模式序列不得漏成可见字符。
/// 漏字的表现就是屏幕上出现 `[38;5;196m` 这类噪声。
#[test]
fn sgr_colours_and_private_modes_leave_no_visible_garbage() {
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(
        tab,
        b"\x1b[2J\x1b[H\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[1;4;7m\x1b[38;5;196m\x1b[48;2;10;20;30m\x1b[0m\x1b[?25h",
    );
    assert!(
        term_diag::term_screen_text(tab).is_empty(),
        "纯控制序列被当成正文画出来了：{:?}",
        term_diag::term_screen_text(tab)
    );
    term_diag::term_feed_inject(
        tab,
        b"\x1b[31mRED\x1b[0m \x1b[38;5;46m256\x1b[0m \x1b[48;2;9;9;9mRGB\x1b[0m",
    );
    assert_eq!(term_diag::term_screen_text(tab), "RED 256 RGB");
    assert_eq!(term_diag::term_stats(tab).cursor_col, 11);
    // 关掉鼠标模式后帧缓冲仍要出得来（花屏冒烟）。
    term_diag::term_feed_inject(tab, b"\x1b[?1006l\x1b[?1000l");
    assert!(term::term_frame(tab).width > 0);
    term::term_close_tab(tab);
}

/// ESC-04 进度条：同一行反复 `\r` 回写 + `ESC[K` 擦尾，屏上只能剩最后一帧。
/// `curl`/`wget` 进度与滚动日志是最常见的残影来源。
#[test]
fn progress_bar_redraws_in_place_and_leaves_no_ghost_rows() {
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[H\x1b[?25l");
    const WIDTH: usize = 20;
    for i in 0..=WIDTH {
        let bar = format!(
            "\r[{}{}] {:>3}%\x1b[K",
            "#".repeat(i),
            ".".repeat(WIDTH - i),
            i as u32 * 100 / WIDTH as u32
        );
        term_diag::term_feed_inject(tab, bar.as_bytes());
    }
    term_diag::term_feed_inject(tab, b"\x1b[?25h\r\x1b[KDONE\r\n");
    let rows = rows_of(tab);
    assert_eq!(rows.len(), 1, "进度条溢出了行数：{rows:?}");
    assert_eq!(rows[0], "DONE", "同一行回写后留下残影");
    term::term_close_tab(tab);
}

/// ESC-03/04 双宽字形按**格**推进而非按字节：CJK / 全角标点 = 2 格，制表符 = 1 格。
/// 错了的表现是中英混排串行、表格竖线错位。
#[test]
fn wide_glyphs_advance_cells_not_bytes() {
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, "\x1b[2J\x1b[H中a中".as_bytes());
    assert_eq!(rows_of(tab)[0], "中a中");
    assert_eq!(
        term_diag::term_stats(tab).cursor_col,
        5,
        "「中」占 2 格：中a中 应走到第 6 格"
    );

    term_diag::term_feed_inject(tab, "\x1b[H─│xＡ".as_bytes());
    assert_eq!(rows_of(tab)[0], "─│xＡ");
    assert_eq!(
        term_diag::term_stats(tab).cursor_col,
        5,
        "制表符 1 格 + 半角 1 格 + 全角 2 格 = 5"
    );
    // 混排后仍然对齐：覆盖写必须落在双宽字算出来的正确格位上。
    term_diag::term_feed_inject(tab, b"\x1b[1;3HZ");
    assert_eq!(rows_of(tab)[0], "─│ZＡ", "双宽序列后的定位");
    assert_eq!(rows_of(tab)[0].chars().nth(2), Some('Z'));
    term::term_close_tab(tab);
}

/// RESIZE-01 自动换行：写超过列宽不丢字、不错格。
#[test]
fn autowrap_beyond_grid_width_wraps_without_losing_characters() {
    let tab = term::term_new_tab();
    let cols = term_diag::term_stats(tab).cols as usize;
    let line: String = (0..cols + 10)
        .map(|i| (b'0' + (i % 10) as u8) as char)
        .collect();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[H");
    term_diag::term_feed_inject(tab, line.as_bytes());
    let rows = rows_of(tab);
    assert_eq!(rows.len(), 2, "{cols} 列写 {} 字应占 2 行", cols + 10);
    assert_eq!(rows[0], &line[..cols], "首行没填满列宽");
    assert_eq!(rows[1], &line[cols..], "换行后的余字不对");
    let st = term_diag::term_stats(tab);
    assert_eq!(
        (st.cursor_col, st.cursor_row),
        (10, 1),
        "换行后光标应落在第二行第 11 格"
    );
    term::term_close_tab(tab);
}

/// RESIZE-02 窗口自适应：缩窗重排不搅乱、不蒸发，被顶出视口的行进回滚区可寻回。
///
/// 实测（本机 100 列 → 60 列）：视口读到的**始终是全文的连续后缀**，重排多占的行把
/// 内容顶端推进历史（`term_mem_report` 显示 历史3行）；探针试过先 `ESC[H` 把光标
/// 归位，结果一字不差，说明引擎按「保住末行」而非「保住光标行」锚定。所以判据不能
/// 要求视口文本「等于全文」（那会把滚动误判成丢字），得拆成三条：连续后缀 +
/// 滚到顶能找回开头 + 放大后逐字复原。
#[test]
fn resize_reflow_wraps_and_restores_text_without_loss() {
    let _serial = input_lock_guard();
    let tab = term::term_new_tab();
    let wide = term_diag::term_stats(tab);
    let (cols, rows) = (wide.cols as usize, wide.rows as usize);
    let (l1, l2, expect) = wrapped_pair(cols);

    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[H");
    term_diag::term_feed_inject(tab, format!("{l1}\r\n{l2}\r\nTAIL").as_bytes());
    assert_eq!(
        compact(&term_diag::term_screen_text(tab)),
        expect,
        "重排前基线就不对"
    );
    let before_lines = rows_of(tab).len();

    let (narrow, _) = resize_cells(tab, cols * 3 / 5, rows);
    assert!((narrow as usize) < cols, "缩窗未生效：{cols} → {narrow}");
    let visible = compact(&term_diag::term_screen_text(tab));
    assert!(
        expect.ends_with(&visible),
        "缩窗把内容搅乱了——视口不是全文的连续后缀：{visible}"
    );
    assert!(
        rows_of(tab).len() >= before_lines,
        "列数变窄却没多占行，内容被裁掉了"
    );

    // 少掉的那段是滚走了、不是没了：Shift+Home 滚到顶应能看见全文开头。
    assert!(
        term::term_key(tab, 5, 1, "").is_empty(),
        "Shift+Home 不得编码成 pty 输入"
    );
    assert!(
        term_diag::term_scroll_offset(tab) > 0,
        "缩窗少掉的内容没进回滚区（是真丢了）"
    );
    let top = compact(&term_diag::term_screen_text(tab));
    assert!(
        top.starts_with(&expect[..40]),
        "滚到顶看不到全文开头：{top}"
    );
    term::term_key(tab, 6, 1, "");
    assert_eq!(term_diag::term_scroll_offset(tab), 0, "Shift+End 没回到底");

    resize_cells(tab, cols, rows);
    let restored = compact(&term_diag::term_screen_text(tab));
    assert_eq!(restored, expect, "放大后未复原（残字/错格/丢行）");
    assert_eq!(rows_of(tab).len(), before_lines, "放大后行数没回到原布局");
    assert!(term::term_frame(tab).width > 0);
    term::term_close_tab(tab);
}

/// RESIZE-03 缩窗后末行不许跳：光标停在末行（= 真实提示符位置）时缩窗，
/// 视口读到的必须是全文的**连续后缀**且末行仍占同一行——被顶出去的是最上面的行，
/// 进了回滚区。这条把用户报的「缩窗后上面的内容不见了」分流成"滚动"而不是"丢字"。
#[test]
fn resize_reflow_keeps_the_last_line_visible() {
    let tab = term::term_new_tab();
    let wide = term_diag::term_stats(tab);
    let (cols, rows) = (wide.cols as usize, wide.rows as usize);
    let (l1, l2, expect) = wrapped_pair(cols);

    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[H");
    term_diag::term_feed_inject(tab, format!("{l1}\r\n{l2}\r\nTAIL").as_bytes());
    // 光标留在最后一行（= 真实 shell 提示符的位置），不归位。
    let cursor_row = term_diag::term_stats(tab).cursor_row;

    let (narrow, _) = resize_cells(tab, cols * 3 / 5, rows);
    let after = rows_of(tab);
    assert_eq!(
        after.len(),
        5,
        "光标行以上被顶出去的行数应与重排多占的行数相等"
    );
    assert_eq!(after[4], "TAIL", "末行内容被打乱");
    assert_eq!(
        term_diag::term_stats(tab).cursor_row,
        cursor_row,
        "重排没有锚在光标行上（提示符会跳位）"
    );
    let visible = compact(&term_diag::term_screen_text(tab));
    assert!(
        expect.ends_with(&visible),
        "视口内容不是全文后缀——中间被搅乱了：narrow={narrow} {visible}"
    );
    term::term_close_tab(tab);
}
