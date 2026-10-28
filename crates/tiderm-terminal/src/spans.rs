//! 「无像素」绘制记录流 —— 原生字形绘制路径的数据面（替代 [`crate::render`] 的整屏 CPU 缓冲）。
//!
//! 软件渲染器把整张网格烘焙成 `w×h×4` 的 RGBA 帧（每窗格两份全屏缓冲），UI 侧只是把
//! 位图贴上去 —— 字形质量因此被 swash 的灰度 AA 锁死，内存随窗格数线性膨胀。本模块把
//! **同一套遍历**（背景 → 字形 → 下划线 → 光标）改为输出绘制记录，交给 UI 自己的文本栈
//! （Qt 壳走 DirectWrite：真实粗/斜字面、ClearType 子像素、按字符的系统回退），Rust 侧
//! 不再产生任何像素缓冲。
//!
//! # 线格式
//! 扁平 `Vec<u32>`，每条记录 [`REC_STRIDE`] 个字，小端语义由宿主按本机序读取：
//!
//! | 字 | 含义 |
//! |---|---|
//! | 0 | `kind`（低 8 位，见 [`KIND_RECT`] / [`KIND_TEXT`]）\| `flags`（次 8 位） |
//! | 1 | `x`（低 16）\| `y`（高 16）——**物理像素**，文本记录为绘制原点 |
//! | 2 | `w`（低 16）\| `h`（高 16）——矩形尺寸；文本记录恒为 0 |
//! | 3 | 颜色 `R | G<<8 | B<<16 | A<<24` |
//! | 4 | 文本：Unicode 码点；矩形：0 |
//! | 5 | 预留（0） |
//!
//! 坐标一律**物理像素**（与 [`crate::surface`] 的帧同域）：网格度量是
//! `FONT_SIZE × dpr` 取整出来的物理单元，若改传逻辑坐标会在非整数 dpr 下逐列错位。
//! 宿主绘制前除以 dpr 即可 —— 整数物理坐标经 `×dpr` 回到设备像素，仍是整数栅格。
//!
//! 记录顺序即绘制顺序（后画者覆盖），与软件渲染器的 pass 顺序严格一致，两条路径的
//! 观感差异才可归因到文本栈本身。

use crate::grid::GridSnapshot;
use crate::render::{gutter_cells, RenderOpts};

/// 一条记录占的 u32 字数。
pub const REC_STRIDE: usize = 6;

/// 实心矩形（背景块、下划线、光标带）。
pub const KIND_RECT: u32 = 0;
/// 单个字形。
pub const KIND_TEXT: u32 = 1;

/// 文本记录：改用粗体字面。
pub const FLAG_BOLD: u32 = 1;
/// 文本记录：改用斜体字面。
pub const FLAG_ITALIC: u32 = 2;

/// 单元格几何（物理像素）。从 `Font` 取，但**不触碰字形缓存** —— 本模块只输出坐标，
/// 一旦为居中宽字符去查 swash 度量，"零像素、零字形缓冲" 的立论就没了。
pub struct Metrics {
    pub cell_w: u32,
    pub cell_h: u32,
    pub ascent: i32,
}

#[inline]
fn rect(out: &mut Vec<u32>, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    out.push(KIND_RECT);
    out.push(pack_xy(x, y));
    out.push(pack_wh(w, h));
    out.push(pack_rgba(color));
    out.push(0);
    out.push(0);
}

/// 文本记录。`x`/`y` 为**绘制原点**：x 是单元格左缘，y 是基线（宿主按 QChar 单字绘制，
/// 由文本引擎自己的 bearing 决定字形在框内的位置）。
#[inline]
fn text(out: &mut Vec<u32>, x: u32, y: u32, color: [u8; 4], ch: char, bold: bool, italic: bool) {
    let flags = (if bold { FLAG_BOLD } else { 0 }) | (if italic { FLAG_ITALIC } else { 0 });
    out.push(KIND_TEXT | flags << 8);
    out.push(pack_xy(x, y));
    out.push(0);
    out.push(pack_rgba(color));
    out.push(ch as u32);
    out.push(0);
}

#[inline]
fn pack_xy(x: u32, y: u32) -> u32 {
    (x.min(u16::MAX as u32)) | (y.min(u16::MAX as u32)) << 16
}

#[inline]
fn pack_wh(w: u32, h: u32) -> u32 {
    (w.min(u16::MAX as u32)) | (h.min(u16::MAX as u32)) << 16
}

#[inline]
fn pack_rgba(c: [u8; 4]) -> u32 {
    (c[0] as u32) | (c[1] as u32) << 8 | (c[2] as u32) << 16 | (c[3] as u32) << 24
}

/// 采集整张可见网格。空单元格不出记录（终端屏幕半数以上是空格），与默认背景同色的
/// 单元格整段跳过 —— 背景按行合并成尽量少的矩形。
pub fn collect(grid: &GridSnapshot, m: &Metrics, opts: &RenderOpts) -> Vec<u32> {
    let cw = m.cell_w;
    let ch = m.cell_h;
    let baseline = |row: usize| (row as i32 * ch as i32 + m.ascent).max(0) as u32;
    // Gutter：整格的行号数字，内容一律右移 gw 像素（与软件渲染器同一条规则）。
    let gw = if opts.gutter {
        gutter_cells(grid.buffer_lines) as u32 * cw
    } else {
        0
    };
    let surf_w = grid.cols as u32 * cw + gw;
    let surf_h = grid.rows as u32 * ch;

    let mut out = Vec::with_capacity((grid.cols * grid.rows / 4).max(1024) * REC_STRIDE);
    let bg0 = grid.default_bg;
    rect(&mut out, 0, 0, surf_w, surf_h, bg0);

    if gw > 0 {
        let dim = crate::render::gutter_color(grid.default_fg, grid.default_bg);
        let gcells = (gw / cw) as usize;
        let width = gcells.saturating_sub(1);
        for (row, &n) in grid
            .row_numbers
            .iter()
            .enumerate()
            .filter(|(_, n)| **n != 0)
        {
            let s = format!("{:>width$}", n, width = width);
            for (di, dch) in s.chars().enumerate() {
                if dch == ' ' {
                    continue;
                }
                text(
                    &mut out,
                    di as u32 * cw,
                    baseline(row),
                    dim,
                    dch,
                    false,
                    false,
                );
            }
        }
    }

    for row in 0..grid.rows {
        let y0 = row as u32 * ch;
        let mut col = 0;
        // 背景合并：与 base 同色/透明的格子不出记录，其余按连续同色段出一个矩形。
        while col < grid.cols {
            let cell = grid.cell(col, row);
            // 宽字符占两格，其右半 spacer 由同一个矩形覆盖（不单独出记录）。
            let cells = if cell.wide { 2 } else { 1 };
            if cell.wide_spacer || cell.bg[3] == 0 || cell.bg == bg0 {
                col += 1;
                continue;
            }
            let start = col;
            let mut span = cells;
            col += cells;
            while col < grid.cols {
                let next = grid.cell(col, row);
                if next.wide || next.wide_spacer || next.bg[3] == 0 || next.bg != cell.bg {
                    break;
                }
                span += 1;
                col += 1;
            }
            rect(
                &mut out,
                gw + start as u32 * cw,
                y0,
                span as u32 * cw,
                ch,
                cell.bg,
            );
        }
        // 字形：宽字符占两格（右半是 spacer，跳过），绘制原点仍取左格左缘。
        for col in 0..grid.cols {
            let cell = grid.cell(col, row);
            if cell.wide_spacer || cell.ch == ' ' || cell.ch == '\0' {
                continue;
            }
            text(
                &mut out,
                gw + col as u32 * cw,
                baseline(row),
                cell.fg,
                cell.ch,
                cell.bold,
                cell.italic,
            );
        }
        // 下划线：与软件渲染器同一条 1 px 带（基线下方 2 px，夹在本行内）。
        for col in 0..grid.cols {
            let cell = grid.cell(col, row);
            if !cell.underline || cell.wide_spacer {
                continue;
            }
            let uy = (y0 as i32 + m.ascent + 2).clamp(y0 as i32, (y0 + ch) as i32 - 1) as u32;
            let w = if cell.wide { cw * 2 } else { cw };
            rect(&mut out, gw + col as u32 * cw, uy, w, 1, cell.fg);
        }
    }

    if grid.cursor_visible && opts.cursor_on && grid.cols > 0 && grid.rows > 0 {
        let (col, row) = grid.cursor;
        let ccol = col.min(grid.cols - 1);
        let crow = row.min(grid.rows - 1);
        let cell = *grid.cell(ccol, crow);
        let x0 = gw + ccol as u32 * cw;
        let y0 = crow as u32 * ch;
        let block_w = if cell.wide { cw * 2 } else { cw };
        // 光标下的字改用它**底下**的颜色画 —— 反相观感来自"换色重画"，不是混合。
        let under = if cell.bg[3] > 0 && cell.bg != bg0 {
            cell.bg
        } else {
            bg0
        };
        match opts.cursor_style {
            1 => {
                let uh = (ch / 5).max(2);
                rect(&mut out, x0, y0 + ch - uh.min(ch), block_w, uh, cell.fg);
            }
            2 => {
                let bw = (cw / 5).max(2);
                rect(&mut out, x0, y0, bw, ch, cell.fg);
            }
            _ => {
                rect(&mut out, x0, y0, block_w, ch, cell.fg);
                if cell.ch != ' ' && cell.ch != '\0' {
                    text(
                        &mut out,
                        x0,
                        baseline(crow),
                        under,
                        cell.ch,
                        cell.bold,
                        cell.italic,
                    );
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{GridSnapshot, RenderCell};

    const CW: u32 = 8;
    const CH: u32 = 17;
    const ASC: i32 = 14;
    const BG: [u8; 4] = [0x10, 0x20, 0x30, 0xff];
    const RED: [u8; 4] = [0xff, 0x00, 0x00, 0xff];

    fn grid(cols: usize, rows: usize) -> GridSnapshot {
        GridSnapshot {
            cols,
            rows,
            cells: vec![RenderCell::default(); cols * rows],
            cursor: (0, 0),
            cursor_visible: false,
            default_bg: BG,
            default_fg: [0xee, 0xee, 0xee, 0xff],
            buffer_lines: 0,
            row_numbers: Vec::new(),
        }
    }

    fn opts() -> RenderOpts {
        RenderOpts {
            cursor_on: true,
            cursor_style: 0,
            gutter: false,
        }
    }

    fn metrics() -> Metrics {
        Metrics {
            cell_w: CW,
            cell_h: CH,
            ascent: ASC,
        }
    }

    /// 记录流 → (kind_word, x, y, w, h, color, codepoint)。kind_word 低 8 位是 kind、
    /// 次 8 位是 flags —— 断言里各自取位，别在 helper 里丢信息。
    fn rows(out: &[u32]) -> Vec<(u32, u32, u32, u32, u32, u32, u32)> {
        out.as_chunks::<REC_STRIDE>()
            .0
            .iter()
            .map(|r| {
                (
                    r[0],
                    r[1] & 0xFFFF,
                    r[1] >> 16,
                    r[2] & 0xFFFF,
                    r[2] >> 16,
                    r[3],
                    r[4],
                )
            })
            .collect()
    }

    fn is_rect(r: &(u32, u32, u32, u32, u32, u32, u32)) -> bool {
        r.0 & 0xFF == KIND_RECT
    }

    #[test]
    fn blank_grid_is_a_single_base_rect() {
        let out = collect(&grid(4, 2), &metrics(), &opts());
        let r = rows(&out);
        assert_eq!(r.len(), 1, "空格子不出文本记录");
        assert!(is_rect(&r[0]));
        assert_eq!((r[0].1, r[0].2, r[0].3, r[0].4), (0, 0, 4 * CW, 2 * CH));
        assert_eq!(r[0].5, pack_rgba(BG));
    }

    #[test]
    fn text_record_carries_cell_box_and_style_flags() {
        let mut g = grid(4, 2);
        let i = g.cols + 1; // (row 1, col 1) 的扁平下标
        g.cells[i].ch = 'A';
        g.cells[i].bold = true;
        g.cells[i].italic = true;
        let r = rows(&collect(&g, &metrics(), &opts()));
        let text = r
            .iter()
            .find(|x| x.0 & 0xFF == KIND_TEXT)
            .expect("文本记录");
        assert_eq!(text.0 >> 8, FLAG_BOLD | FLAG_ITALIC);
        assert_eq!((text.1, text.2), (CW, CH + ASC as u32), "x=格左缘，y=基线");
        assert_eq!(text.6, 'A' as u32);
    }

    #[test]
    fn same_bg_coalesces_and_default_bg_is_skipped() {
        let mut g = grid(4, 1);
        for c in 0..3 {
            g.cells[c].bg = RED;
        }
        g.cells[3].bg = BG; // 与默认背景同色 ⇒ 底矩形已覆盖
        let r = rows(&collect(&g, &metrics(), &opts()));
        let rects: Vec<_> = r.iter().filter(|x| is_rect(x)).collect();
        assert_eq!(rects.len(), 2, "底矩形 + 一段合并背景");
        assert_eq!((rects[1].1, rects[1].3), (0, 3 * CW));
        assert_eq!(rects[1].5, pack_rgba(RED));
    }

    #[test]
    fn wide_char_emits_one_glyph_over_two_cells() {
        let mut g = grid(4, 1);
        g.cells[0].ch = '中';
        g.cells[0].wide = true;
        g.cells[0].bg = RED;
        g.cells[1].wide_spacer = true;
        g.cells[1].bg = RED;
        let r = rows(&collect(&g, &metrics(), &opts()));
        assert_eq!(r.iter().filter(|x| x.0 & 0xFF == KIND_TEXT).count(), 1);
        let rects: Vec<_> = r.iter().filter(|x| is_rect(x)).collect();
        assert_eq!(rects[1].3, 2 * CW, "spacer 由同一矩形覆盖");
    }

    #[test]
    fn block_cursor_redraws_glyph_in_the_color_it_covers() {
        let mut g = grid(4, 1);
        g.cells[0].ch = 'A';
        g.cells[0].fg = [0x11, 0x22, 0x33, 0xff];
        g.cells[0].bg = RED;
        g.cursor_visible = true;
        let r = rows(&collect(&g, &metrics(), &opts()));
        // 记录顺序：底矩形 → 背景 → 常规字形 → 光标块矩形 → 反相字形（后画者覆盖）。
        let (block, inverted) = (&r[r.len() - 2], r.last().unwrap());
        assert!(is_rect(block) && block.5 == pack_rgba([0x11, 0x22, 0x33, 0xff]));
        assert_eq!(inverted.0 & 0xFF, KIND_TEXT);
        assert_eq!(inverted.5, pack_rgba(RED), "块内字形改用其底下的颜色");
        assert_eq!(inverted.6, 'A' as u32);
    }
}
