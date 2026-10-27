//! Qt owns clipboard access; terminal state never opens a platform clipboard handle.
pub(super) struct QtClipboard;
pub(super) fn clipboard() -> Box<dyn tiderm_terminal::host::ClipboardAccess> {
    if crate::ffi::qt_has_gui_application() {
        Box::new(QtClipboard)
    } else {
        Box::<HeadlessClipboard>::default()
    }
}

/// A pane-local clipboard for the supported headless diagnostics. It never opens
/// a desktop connection, so G1 and Rust tests cannot read or overwrite user data.
#[derive(Default)]
struct HeadlessClipboard(String);
impl tiderm_terminal::host::ClipboardAccess for HeadlessClipboard {
    fn copy(&mut self, text: &str) -> bool {
        self.0 = text.into();
        true
    }
    fn paste(&mut self) -> Option<String> {
        Some(self.0.clone())
    }
    fn has_image(&mut self) -> bool {
        false
    }
}
impl tiderm_terminal::host::ClipboardAccess for QtClipboard {
    fn copy(&mut self, text: &str) -> bool {
        crate::ffi::qt_clipboard_copy(text)
    }
    fn paste(&mut self) -> Option<String> {
        let text = crate::ffi::qt_clipboard_read();
        (!text.is_empty()).then_some(text)
    }
    fn has_image(&mut self) -> bool {
        crate::ffi::qt_clipboard_has_image()
    }
}
