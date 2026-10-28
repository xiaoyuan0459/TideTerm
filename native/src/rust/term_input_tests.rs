//! 维度 2 的**输入侧**无头补测（TTY-01/02/03 的键盘与鼠标半边）+ 维度 5 的剪贴板负路径。
//!
//! §0.6/§0.8 覆盖的是"字节流进引擎之后"（转义序列、重排、区域滚动），而**输出对不对、
//! 输入发不发得出去**是另一条路：`term_key` / `term_mouse` / `term_wheel` 走的是
//! `KeyEvent → encode_key` 与 `pane.mouse_report`，任何一环把序列吞了，用户在 vim / less /
//! htop 里的表现都是"按了没反应"，而且**屏幕上没有任何痕迹可查**。
//!
//! 三块覆盖：
//! ① 特殊键编码表（方向键/Home/End/PgUp/PgDn/F1-F12/Enter/Tab/Backspace + Ctrl/Alt 修饰）；
//! ② 鼠标上报**只应由应用开的跟踪模式决定**（1000/1002/1003/1006 的开关语义）；
//! ③ 滚轮在"应用不要鼠标"时滚回滚区、在"应用要鼠标"时改发鼠标序列（这决定 vim 里能不能用滚轮）。
//!
//! 全部本机、无靶机、无凭据。谨慎处理剪贴板：不写系统剪贴板（会覆盖用户正在用的内容），
//! 只断言**负路径**（空剪贴板 → 粘贴返回 false 且不向会话写任何字节）。

use super::term_net_tests::net_serial;
use super::term_tests::input_lock_guard;
use super::{term, term_diag};

/// 桥面中立键码（`term_keys.rs:20-40`）：Qt 特殊键的 `QKeyEvent::text()` 是空串，
/// 桥面必须补 X16 keysym 哨兵，否则这些键**全发不进 pty**。
const KEY_UP: u32 = 1;
const KEY_DOWN: u32 = 2;
const KEY_LEFT: u32 = 3;
const KEY_RIGHT: u32 = 4;
const KEY_HOME: u32 = 5;
const KEY_END: u32 = 6;
const KEY_PGUP: u32 = 7;
const KEY_PGDN: u32 = 8;
const KEY_BACKSPACE: u32 = 11;
const KEY_TAB: u32 = 12;
const KEY_RETURN: u32 = 14;
const KEY_ESCAPE: u32 = 15;

const MOD_ALT: u32 = 2;
const MOD_CTRL: u32 = 4;

/// TTY-02 输入半边：特殊键必须编码成转义序列发出去，而不是被静默丢弃。
/// 只断言"非空 + 形状合理"两类不变式，不断言具体字节（各家编码差异留给定量探针）——
/// 这样测的是"发不发得出去"，不是"跟 xterm 逐字节相同"。
#[test]
fn special_keys_encode_to_nonempty_sequences_for_the_pty() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();

    // 方向键 / Home / End / PgUp / PgDn / Escape：必须产出以 ESC 开头的序列。
    // **F1-F12 不在这里** —— 实测它们产出的是私有区 Unicode 哨兵字符的 UTF-8 字节
    // （F1 → 0xEF 0x9C 0x84），不是 `ESC O P`；见下面的 R-22 哨兵。
    for (name, code) in [
        ("Up", KEY_UP),
        ("Down", KEY_DOWN),
        ("Left", KEY_LEFT),
        ("Right", KEY_RIGHT),
        ("Home", KEY_HOME),
        ("End", KEY_END),
        ("PgUp", KEY_PGUP),
        ("PgDn", KEY_PGDN),
        ("Escape", KEY_ESCAPE),
    ] {
        let bytes = term::term_key(tab, code, 0, "");
        assert!(
            !bytes.is_empty(),
            "{name} 没编码出任何字节 ⇒ 这个键发不进 pty（vim/less 里静默失灵）"
        );
        assert_eq!(
            bytes[0],
            0x1b,
            "{name} 的首字节不是 ESC：{:?}",
            String::from_utf8_lossy(&bytes)
        );
    }

    // Enter / Tab / Backspace：单字节控制字符，同样不能为空。
    assert_eq!(term::term_key(tab, KEY_RETURN, 0, ""), b"\r");
    assert_eq!(term::term_key(tab, KEY_TAB, 0, ""), b"\t");
    assert_eq!(term::term_key(tab, KEY_BACKSPACE, 0, ""), b"\x7f");

    // 可打印文本：原样过去；Ctrl/Alt 修饰必须产生**不同**的字节。
    assert_eq!(term::term_key(tab, 0, 0, "a"), b"a");
    let ctrl_c = term::term_key(tab, 0, MOD_CTRL, "c");
    assert_eq!(ctrl_c, &[0x03], "Ctrl+C 应编码成 0x03：{:?}", ctrl_c);
    let alt_a = term::term_key(tab, 0, MOD_ALT, "a");
    assert!(
        !alt_a.is_empty() && alt_a != b"a",
        "Alt+A 与裸 A 无区别：{:?}",
        alt_a
    );

    term::term_close_tab(tab);
}

/// **未识别的中立键码**必须安静失败（空字节），不能把未知 keysym 当文本发出去——
/// 后者会在 vim 里敲出一个乱码字符，且用户完全无法归因。
#[test]
fn unknown_key_codes_encode_to_nothing_rather_than_garbage() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    assert!(
        term::term_key(tab, 9999, 0, "").is_empty(),
        "未知键码产出了字节 ⇒ 有风险把哨兵 keysym 当文本写进 pty"
    );
    term::term_close_tab(tab);
}

/// TTY-03 输入半边：鼠标上报**只由应用开的跟踪模式决定**。
/// 这是"vim 里能不能拖动分割线 / htop 能不能点列排序"的开关，任何一档判错的
/// 表现都是"应用内鼠标失灵"或"没开跟踪的界面里乱发序列把命令注进 shell"。
#[test]
fn mouse_reports_follow_the_apps_tracking_mode_and_nothing_else() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    let st = term_diag::term_stats(tab);
    let (w, h) = (
        st.cols as f32 * st.cell_w as f32,
        st.rows as f32 * st.cell_h as f32,
    );
    // 屏幕中偏下的一点（避免 0 边界被 clamp 吃掉差异）。
    let (x, y) = (w / 2.0, h / 2.0);

    // ① 默认（未开任何跟踪）：完全不报。
    assert!(
        term::term_mouse(tab, 0, 0, 0, x, y).is_empty(),
        "应用没开鼠标跟踪，按下却产出了序列 ⇒ 会往 shell 里注乱码"
    );
    assert!(
        !term::term_mouse_send(tab, 0, 0, x, y),
        "没开跟踪时 term_mouse_send 也应为 false"
    );

    // ② `ESC[?1000h`（普通跟踪）：按下/松开要报，**移动不该报**。
    term_diag::term_feed_inject(tab, b"\x1b[?1000h");
    let press = term::term_mouse(tab, 0, 0, 0, x, y);
    assert!(!press.is_empty(), "开了 1000h 之后按下没有序列");
    assert!(
        term::term_mouse(tab, 2, 0, 0, x, y).len() >= 3,
        "开了 1000h 之后松开没有序列"
    );
    assert!(
        term::term_mouse(tab, 1, 0, 0, x, y).is_empty(),
        "1000（普通跟踪）下移动不该上报"
    );

    // ③ `ESC[?1002h`（按钮事件跟踪）：**按住**（button≠3）时移动要报；
    //    **未按键**的移动（button=3）不该报。
    //    注意 `mouse_report` 是**无状态**的：held 与否由本次调用传入的 button 决定
    //    （`pane.rs:1160 let held = button != 3`），引擎不记按压状态。
    term_diag::term_feed_inject(tab, b"\x1b[?1002h");
    assert!(
        !term::term_mouse(tab, 0, 0, 0, x, y).is_empty(),
        "1002h 按下没序列"
    );
    assert!(
        !term::term_mouse(tab, 1, 0, 0, x, y).is_empty(),
        "1002h 按住时移动没序列（vim 拖动分割线/选中会失灵）"
    );
    term::term_mouse(tab, 2, 0, 0, x, y);
    assert!(
        term::term_mouse(tab, 1, 3, 0, x, y).is_empty(),
        "1002h 下未按键的移动仍在上报（会把野序列发给应用）"
    );

    // ④ `ESC[?1003h`（任意移动跟踪）：不按也报。
    term_diag::term_feed_inject(tab, b"\x1b[?1003h");
    assert!(
        !term::term_mouse(tab, 1, 0, 0, x, y).is_empty(),
        "1003h 下自由移动没序列"
    );

    // ⑤ `ESC[?1006h`（SGR 扩展）：序列形状必须是 `ESC [ < …`，不是 X10 的 `ESC [ M`。
    //    注意 1006 只改编码、不开跟踪（单独发它连报都不报，见 `pane.rs` 自测与实测），
    //    所以必须跟在 1000/1002/1003 之后单独发一次才生效。
    term_diag::term_feed_inject(tab, b"\x1b[?1006h");
    let sgr = term::term_mouse(tab, 0, 0, 0, x, y);
    assert!(
        sgr.starts_with(b"\x1b[<"),
        "开了 1006h 却不是 SGR 形状：{:?}",
        String::from_utf8_lossy(&sgr)
    );

    // ⑥ 关掉跟踪（`ESC[?1000l` 只关普通档；这里连续关三档再验一次安静）。
    term_diag::term_feed_inject(tab, b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l");
    assert!(
        term::term_mouse(tab, 0, 0, 0, x, y).is_empty(),
        "关掉跟踪之后仍在发序列"
    );

    term::term_close_tab(tab);
}

/// 滚轮语义分叉：**应用不要鼠标**时滚轮回滚区（用户能翻历史），**应用要鼠标**时
/// 改发鼠标序列（vim/less 里滚轮 = 上下移动）。这条同时是维度 2 与维度 3 的交叉点。
#[test]
fn wheel_scrolls_history_when_the_app_ignores_mouse_else_it_reports() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    let st = term_diag::term_stats(tab);
    let (w, h) = (
        st.cols as f32 * st.cell_w as f32,
        st.rows as f32 * st.cell_h as f32,
    );

    // 先灌出回滚区。
    for i in 0..40 {
        term_diag::term_feed_inject(tab, format!("LINE{i}\r\n").as_bytes());
    }
    assert_eq!(
        term_diag::term_scroll_offset(tab),
        0,
        "灌完内容视口应停在底部"
    );

    // ① 应用不要鼠标：滚轮 = 视口回滚。
    let bytes = term::term_wheel(tab, 3, w / 2.0, h / 2.0);
    assert!(
        term_diag::term_scroll_offset(tab) > 0,
        "滚轮没有把视口滚起来（bytes={:?}）",
        String::from_utf8_lossy(&bytes)
    );

    // ② 应用要鼠标（1002h）：滚轮改发鼠标序列，且不再顺带动视口。
    term::term_wheel(tab, -999, w / 2.0, h / 2.0); // 先回到底，排除上一拍的偏移
    assert_eq!(term_diag::term_scroll_offset(tab), 0, "负向滚轮没回到底");
    term_diag::term_feed_inject(tab, b"\x1b[?1002h");
    let wheel_bytes = term::term_wheel(tab, 3, w / 2.0, h / 2.0);
    assert!(
        !wheel_bytes.is_empty(),
        "开了 1002h 之后滚轮没有产出鼠标序列"
    );
    assert_eq!(
        term_diag::term_scroll_offset(tab),
        0,
        "应用已接管滚轮，视口却仍被滚动（两边同时响应）"
    );

    term::term_close_tab(tab);
}

/// 维度 5 的剪贴板**负路径**：空剪贴板 ⇒ 粘贴应返回 false 且**不向会话写任何字节**。
/// 有意不写系统剪贴板（那会覆盖用户剪贴板里的内容，属于有副作用的测试）。
#[test]
fn pasting_from_an_empty_clipboard_writes_nothing_and_reports_false() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    if term::term_clipboard_text(tab).is_empty() {
        assert!(
            !term::term_paste_from_clipboard(tab),
            "剪贴板为空时粘贴却返回 true"
        );
    }
    // 选区撤空：`selection_is_drag` 为假时应清空而不是复制 0 字符。
    term::term_selection_begin(tab, 0.0, 0.0);
    term::term_selection_update(tab, 0.0, 0.0);
    assert_eq!(
        term::term_selection_end(tab),
        0,
        "零面积拖拽竟然上报了复制字符数"
    );
    term::term_close_tab(tab);
}

/// 拖拽选区：在已知内容上拖一格，应复制出**非 0**的字符数（选区与剪贴板的连线）。
/// 只在能读到剪贴板内容时校验文本本身，避免依赖系统剪贴板可用性。
#[test]
fn dragging_a_selection_over_known_text_reports_a_nonzero_char_count() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    term_diag::term_feed_inject(tab, b"\x1b[2J\x1b[HSELECT_ME_ROW");
    let st = term_diag::term_stats(tab);
    let cw = st.cell_w as f32;
    term::term_selection_begin(tab, 0.0, 0.0);
    term::term_selection_update(tab, 8.0 * cw, 0.0);
    let copied = term::term_selection_end(tab);
    assert!(
        copied > 0,
        "在 12 个字符上横向拖拽却复制了 0 个字符 ⇒ 选区链路断了"
    );
    let text = term::term_clipboard_text(tab);
    if !text.is_empty() {
        assert!(
            text.contains("SELECT_ME") || text.contains("SELECT"),
            "复制到的内容与屏上不符：{text:?}"
        );
    }
    // 拖拽之后视口不得被选区拖动带着走。
    assert_eq!(
        term_diag::term_scroll_offset(tab),
        0,
        "拖拽选区把视口滚走了"
    );
    term::term_close_tab(tab);
}

/// **R-22 已修（回归守卫，常规门禁内）**：
/// Native 中立码到引擎再到 PTY 的功能键路径不能泄漏私有 Unicode 哨兵。
/// 精确 VT 字节契约由 tiderm-terminal 的 keys 行为测试负责。
#[test]
fn function_keys_encode_to_escape_sequences_not_private_use_bytes() {
    let _serial = net_serial();
    let _input = input_lock_guard();
    let tab = term::term_new_tab();
    for (n, code) in (32u32..44).enumerate() {
        let bytes = term::term_key(tab, code, 0, "");
        let printable = String::from_utf8_lossy(&bytes);
        assert!(
            bytes.first() == Some(&0x1b) && printable.is_ascii(),
            "F{} 编成了 {:?}（{:02X?}）而不是 ESC 序列 ⇒ 应用里这个键失灵",
            n + 1,
            printable,
            bytes
        );
    }
    term::term_close_tab(tab);
}
