use super::*;
use crate::{SoftwareRenderer, TerminalPane};
use std::{cell::RefCell, rc::Rc};

struct HostClipboard(Rc<RefCell<String>>);
impl ClipboardAccess for HostClipboard {
    fn copy(&mut self, text: &str) -> bool {
        *self.0.borrow_mut() = text.into();
        true
    }
    fn paste(&mut self) -> Option<String> {
        Some(self.0.borrow().clone())
    }
    fn has_image(&mut self) -> bool {
        false
    }
}

#[test]
fn clipboard_delivery_is_explicit_and_bound_to_its_pane() {
    let mut first = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    let mut second = TerminalPane::new(20, 4, Box::new(SoftwareRenderer::new()));
    let data = Rc::new(RefCell::new(String::new()));
    first.set_clipboard(Box::new(HostClipboard(data.clone())));
    first.feed_bytes(b"\x1b]52;c;Zml4dHVyZQ==\x07");
    assert_eq!(*data.borrow(), "fixture");
    assert_eq!(first.paste_from_clipboard().as_deref(), Some("fixture"));
    assert!(second.paste_from_clipboard().is_none());
    second.feed_bytes(b"\x1b]52;c;b3RoZXI=\x07");
    assert_eq!(*data.borrow(), "fixture");
    first.feed_bytes(b"\x1b]52;c;?\x07");
    let reply = String::from_utf8(first.take_replies()).unwrap();
    assert!(reply.starts_with("\x1b]52;c;"));
    assert!(reply.contains("Zml4dHVyZQ=="));
}
