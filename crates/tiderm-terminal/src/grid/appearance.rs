//! Resolve cell appearance once, then project it to rendering or stored runs.
use super::{dim_blend, pack_rgba, Cell, Flags, RenderCell, Rgb, StyleRun, TermGrid};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CellStyle {
    foreground: [u8; 4],
    background: [u8; 4],
    bold: bool,
    italic: bool,
    underline: bool,
    wide: bool,
}

impl CellStyle {
    pub(super) fn is_wide(self) -> bool {
        self.wide
    }

    /// Whether this cell paints an underline (which shows even under a space).
    pub(super) fn underline(self) -> bool {
        self.underline
    }

    /// Whether this cell paints a background (the default one is fully transparent, so a
    /// space on it looks exactly like an absent cell — see `line_data_from_grid_row`).
    pub(super) fn background_visible(self) -> bool {
        self.background[3] != 0
    }

    pub(super) fn render(self, ch: char, spacer: bool) -> RenderCell {
        RenderCell {
            ch,
            fg: self.foreground,
            bg: self.background,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            wide: self.wide,
            wide_spacer: spacer,
        }
    }

    pub(super) fn start_run(self, column: u16) -> StyleRun {
        StyleRun {
            start: column,
            len: 1,
            fg: pack_rgba(self.foreground),
            bg: pack_rgba(self.background),
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            wide: self.wide,
        }
    }
}

impl TermGrid {
    pub(super) fn cell_style(&self, cell: &Cell) -> CellStyle {
        let mut foreground = resolve(&self.palette, cell.fg, true);
        let mut background = resolve(&self.palette, cell.bg, false);
        if cell.flags.contains(Flags::INVERSE) {
            // Swap actual colours before introducing the transparent default-bg
            // marker; an inverted default background becomes an opaque glyph.
            std::mem::swap(&mut foreground, &mut background);
        } else if matches!(cell.bg, Color::Named(NamedColor::Background)) {
            background = [0; 4];
        }
        if cell.flags.contains(Flags::DIM) {
            let backdrop = if background[3] == 0 {
                resolve(&self.palette, Color::Named(NamedColor::Background), false)
            } else {
                background
            };
            foreground = dim_blend(foreground, backdrop);
        }
        CellStyle {
            foreground,
            background,
            bold: cell.flags.contains(Flags::BOLD),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: cell
                .flags
                .intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE),
            wide: cell.flags.contains(Flags::WIDE_CHAR),
        }
    }

    pub(super) fn render_cell(&self, cell: &Cell) -> RenderCell {
        self.cell_style(cell)
            .render(cell.c, cell.flags.contains(Flags::WIDE_CHAR_SPACER))
    }
}

fn resolve(palette: &[Rgb; 256], color: Color, foreground: bool) -> [u8; 4] {
    let rgb = match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => palette[usize::from(index)],
        Color::Named(name) => {
            let index = match name {
                NamedColor::Foreground => 7,
                NamedColor::Background => 0,
                name if (name as usize) < 256 => (name as usize).min(15),
                _ if foreground => 7,
                _ => 0,
            };
            palette[index]
        }
    };
    [rgb.r, rgb.g, rgb.b, 255]
}
