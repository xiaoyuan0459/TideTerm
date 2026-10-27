//! Host services injected explicitly. An engine without a host cannot access OS data.

pub trait ClipboardAccess {
    fn copy(&mut self, text: &str) -> bool;
    fn paste(&mut self) -> Option<String>;
    fn has_image(&mut self) -> bool;
}

#[derive(Default)]
pub(crate) struct Clipboard {
    access: Option<Box<dyn ClipboardAccess>>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn install(&mut self, access: Box<dyn ClipboardAccess>) {
        self.access = Some(access);
    }
    pub fn copy(&mut self, text: &str) -> bool {
        self.access.as_mut().is_some_and(|access| access.copy(text))
    }
    pub fn paste(&mut self) -> Option<String> {
        self.access
            .as_mut()?
            .paste()
            .filter(|text| !text.is_empty())
    }
    pub fn has_image(&mut self) -> bool {
        self.access
            .as_mut()
            .is_some_and(|access| access.has_image())
    }
}

#[cfg(test)]
mod tests;
