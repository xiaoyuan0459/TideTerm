//! Parser notifications owned by one grid. No worker, transport or host access.
use super::colors::default_palette;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::vte::ansi::Rgb;
use std::sync::{Arc, Mutex};

/// An OSC 52 request delivered to the pane's explicitly supplied host service.
pub enum OscEvent {
    /// Store decoded clipboard text.
    ClipboardStore(String),
    /// Read the clipboard, format it, then queue the reply with `TermGrid::reply_to_pty`.
    ClipboardLoad(Arc<dyn Fn(&str) -> String + Sync + Send + 'static>),
}

struct Pending {
    replies: Vec<u8>,
    clipboard: Vec<OscEvent>,
    title: Option<String>,
    bell: bool,
    base16: [Rgb; 16],
}

/// Shared only between a Term and its owning grid; cloned handles share no other grid.
/// Titles and BEL are notifications, so coalesce at arrival. Protocol bytes and
/// clipboard operations retain every item in their respective arrival order.
#[derive(Clone)]
pub(super) struct Mailbox(Arc<Mutex<Pending>>);

impl Mailbox {
    pub(super) fn new(base16: [Rgb; 16]) -> Self {
        Self(Arc::new(Mutex::new(Pending {
            replies: Vec::new(),
            clipboard: Vec::new(),
            title: None,
            bell: false,
            base16,
        })))
    }

    pub(super) fn set_base16(&self, base16: [Rgb; 16]) {
        self.0.lock().unwrap().base16 = base16;
    }

    pub(super) fn take_replies(&self) -> Vec<u8> {
        std::mem::take(&mut self.0.lock().unwrap().replies)
    }

    pub(super) fn reply(&self, bytes: &[u8]) {
        self.0.lock().unwrap().replies.extend_from_slice(bytes);
    }

    pub(super) fn take_clipboard(&self) -> Vec<OscEvent> {
        std::mem::take(&mut self.0.lock().unwrap().clipboard)
    }

    pub(super) fn take_title(&self) -> Option<String> {
        self.0.lock().unwrap().title.take()
    }

    pub(super) fn take_bell(&self) -> bool {
        std::mem::take(&mut self.0.lock().unwrap().bell)
    }

    fn requested_color(&self, index: usize) -> Rgb {
        let pending = self.0.lock().unwrap();
        match index {
            256 | 258 => pending.base16[7],
            257 => pending.base16[0],
            i if i < 16 => pending.base16[i],
            _ => default_palette()[index.min(255)],
        }
    }
}

impl EventListener for Mailbox {
    fn send_event(&self, event: Event) {
        // Never hold the mailbox lock while invoking a parser-supplied formatter.
        if let Event::ColorRequest(index, format) = event {
            let color = self.requested_color(index);
            self.reply(format(color).as_bytes());
            return;
        }
        let mut pending = self.0.lock().unwrap();
        match event {
            Event::PtyWrite(text) => pending.replies.extend_from_slice(text.as_bytes()),
            Event::ClipboardStore(_, text) => {
                pending.clipboard.push(OscEvent::ClipboardStore(text));
            }
            Event::ClipboardLoad(_, format) => {
                pending.clipboard.push(OscEvent::ClipboardLoad(format));
            }
            Event::Title(title) => pending.title = Some(title),
            Event::ResetTitle => pending.title = Some(String::new()),
            Event::Bell => pending.bell = true,
            _ => {}
        }
    }
}
