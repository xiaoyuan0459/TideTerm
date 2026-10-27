//! Observe executed parser operations, including deferred synchronized output.
//! The parser still owns ANSI acceptance, cancellation, parameters and modes.
use super::{mailbox::Mailbox, projection::Projection, TermGrid};
use alacritty_terminal::vte::ansi::cursor_icon::CursorIcon;
use alacritty_terminal::{grid::Dimensions, term::Term, vte::ansi::*};

#[derive(Default)]
pub(crate) struct FeedResult {
    pub scrolled: usize,
    pub clear_view: bool,
    pub clear_primary: bool,
    pub reset: bool,
}

/// Ordered mutations needed by retained source anchors. No raw byte parser in the host.
pub(crate) enum OutputEvent {
    Scroll(usize),
    Alternate,
    Clear { primary: bool, reset: bool },
    ShellMarker(u8),
}

type Observer<'a> = dyn FnMut(&TermGrid, OutputEvent) + 'a;

struct Output<'a> {
    grid: &'a mut TermGrid,
    result: FeedResult,
    observer: &'a mut Observer<'a>,
}

impl Output<'_> {
    /// Account for executed mutations, including newlines replayed from a
    /// synchronized update, before the next Handler operation runs.
    #[inline]
    fn dispatch(&mut self, apply: impl FnOnce(&mut Term<Mailbox>)) {
        let before = self.grid.term.grid().history_size();
        let alternate = self.grid.alt_screen();
        apply(&mut self.grid.term);
        let primary = !alternate && !self.grid.alt_screen();
        let growth = self.grid.term.grid().history_size().saturating_sub(before);
        if primary && growth != 0 {
            self.result.scrolled = self.result.scrolled.saturating_add(growth);
            let target = self.grid.viewport.after_output(growth, true);
            self.grid.drain_pending();
            if self.grid.viewport.is_held() {
                self.grid.set_view_offset(target);
            }
            (self.observer)(self.grid, OutputEvent::Scroll(growth));
        } else if alternate != self.grid.alt_screen() {
            self.grid.set_view_offset(0);
            (self.observer)(self.grid, OutputEvent::Alternate);
        }
    }

    fn cleared(&mut self, primary: bool, reset: bool) {
        self.result.clear_view = true;
        self.result.clear_primary |= primary;
        self.result.reset |= reset;
        self.result.scrolled = 0;
        self.grid.projection = Projection::default();
        self.grid.set_view_offset(0);
        (self.observer)(self.grid, OutputEvent::Clear { primary, reset });
    }
}

// Keep ordinary Handler operations transparent. The two lifecycle operations
// below additionally retire TideTerm state at the same position in the stream.
macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {$(
        fn $name(&mut self, $($arg: $ty),*) {
            self.dispatch(|term| term.$name($($arg),*));
        }
    )*};
}

impl Handler for Output<'_> {
    fn shell_marker(&mut self, marker: u8) {
        // Anchors read retained source rows. Make deferred rows visible before
        // observing the marker, exactly as they are after an ordinary feed.
        self.grid.flush_pending();
        (self.observer)(self.grid, OutputEvent::Scroll(0));
        (self.observer)(self.grid, OutputEvent::ShellMarker(marker));
    }

    forward! {
        set_title(title: Option<String>);
        set_cursor_style(style: Option<CursorStyle>);
        set_cursor_shape(shape: CursorShape);
        input(c: char);
        goto(line: i32, col: usize);
        goto_line(line: i32);
        goto_col(col: usize);
        insert_blank(count: usize);
        move_up(rows: usize);
        move_down(rows: usize);
        identify_terminal(intermediate: Option<char>);
        device_status(status: usize);
        move_forward(cols: usize);
        move_backward(cols: usize);
        move_down_and_cr(rows: usize);
        move_up_and_cr(rows: usize);
        put_tab(count: u16);
        backspace();
        carriage_return();
        linefeed();
        bell();
        substitute();
        newline();
        set_horizontal_tabstop();
        scroll_up(rows: usize);
        scroll_down(rows: usize);
        insert_blank_lines(count: usize);
        delete_lines(count: usize);
        erase_chars(count: usize);
        delete_chars(count: usize);
        move_backward_tabs(count: u16);
        move_forward_tabs(count: u16);
        save_cursor_position();
        restore_cursor_position();
        clear_line(mode: LineClearMode);
        clear_tabs(mode: TabulationClearMode);
        set_tabs(interval: u16);
        reverse_index();
        terminal_attribute(attr: Attr);
        set_mode(mode: Mode);
        unset_mode(mode: Mode);
        report_mode(mode: Mode);
        set_private_mode(mode: PrivateMode);
        unset_private_mode(mode: PrivateMode);
        report_private_mode(mode: PrivateMode);
        set_scrolling_region(top: usize, bottom: Option<usize>);
        set_keypad_application_mode();
        unset_keypad_application_mode();
        set_active_charset(index: CharsetIndex);
        configure_charset(index: CharsetIndex, charset: StandardCharset);
        set_color(index: usize, color: Rgb);
        dynamic_color_sequence(prefix: String, index: usize, terminator: &str);
        reset_color(index: usize);
        clipboard_store(clipboard: u8, text: &[u8]);
        clipboard_load(clipboard: u8, terminator: &str);
        decaln();
        push_title();
        pop_title();
        text_area_size_pixels();
        text_area_size_chars();
        set_hyperlink(link: Option<Hyperlink>);
        set_mouse_cursor_icon(icon: CursorIcon);
        report_keyboard_mode();
        push_keyboard_mode(mode: KeyboardModes);
        pop_keyboard_modes(count: u16);
        set_keyboard_mode(mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        set_modify_other_keys(mode: ModifyOtherKeys);
        report_modify_other_keys();
        set_scp(path: ScpCharPath, mode: ScpUpdateMode);
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        let primary = !self.grid.alt_screen();
        let clear = matches!(mode, ClearMode::All) || (primary && matches!(mode, ClearMode::Saved));
        self.grid.erase_screen(mode);
        if clear {
            self.cleared(primary, false);
        }
    }

    fn reset_state(&mut self) {
        self.grid.term.reset_state();
        self.grid.retire_history();
        self.cleared(true, true);
    }
}

impl TermGrid {
    /// Parse peer output and apply history retirement at each accepted operation.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.feed_with_scroll(bytes);
    }

    pub(crate) fn feed_with_scroll(&mut self, bytes: &[u8]) -> FeedResult {
        self.feed_observed(bytes, &mut |_, _| {})
    }

    pub(crate) fn feed_observed(
        &mut self,
        bytes: &[u8],
        observer: &mut Observer<'_>,
    ) -> FeedResult {
        if bytes.is_empty() {
            return FeedResult::default();
        }
        // Move, then restore the same Processor, preserving fragmented input and
        // its synchronized buffer. The temporary default never receives bytes.
        let mut parser = std::mem::take(&mut self.parser);
        let mut output = Output {
            grid: self,
            result: FeedResult::default(),
            observer,
        };
        parser.advance(&mut output, bytes);
        let result = output.result;
        self.parser = parser;
        self.flush_pending();
        self.dirty = true;
        result
    }
}
