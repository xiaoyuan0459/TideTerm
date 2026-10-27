//! Immutable text with one shared prefix. No prefix chains or disk access.
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LineText {
    pub(super) prefix: Option<Arc<[u16]>>,
    pub(super) tail: Arc<[u16]>,
}

impl From<Vec<u16>> for LineText {
    fn from(text: Vec<u16>) -> Self {
        Self {
            prefix: None,
            tail: text.into(),
        }
    }
}

impl LineText {
    pub fn len(&self) -> usize {
        self.prefix.as_ref().map_or(0, |p| p.len()) + self.tail.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, col: usize) -> Option<&u16> {
        let prefix = self.prefix.as_deref().unwrap_or_default();
        if col < prefix.len() {
            prefix.get(col)
        } else {
            self.tail.get(col - prefix.len())
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &u16> {
        self.prefix
            .iter()
            .flat_map(|p| p.iter())
            .chain(self.tail.iter())
    }
    pub fn to_arc(&self) -> Arc<[u16]> {
        if self.prefix.is_none() {
            self.tail.clone()
        } else {
            self.iter().copied().collect::<Vec<_>>().into()
        }
    }
    pub(super) fn split_prefix(&self, known: Option<&Arc<[u16]>>) -> Self {
        // Sharing 64 code units saves 128 bytes per similar log row. Short rows
        // keep one allocation, and a changed prefix cannot pin an older row.
        if self.len() < 80 {
            return self.clone();
        }
        let units = self.to_arc();
        Self {
            prefix: Some(known.cloned().unwrap_or_else(|| Arc::from(&units[..64]))),
            tail: Arc::from(&units[64..]),
        }
    }
}

impl PartialEq for LineText {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        match (&self.prefix, &other.prefix) {
            (None, None) => self.tail == other.tail,
            (Some(a), Some(b)) => a == b && self.tail == other.tail,
            (Some(prefix), None) => {
                other.tail.starts_with(prefix) && other.tail[prefix.len()..] == *self.tail
            }
            (None, Some(prefix)) => {
                self.tail.starts_with(prefix) && self.tail[prefix.len()..] == *other.tail
            }
        }
    }
}
impl Eq for LineText {}
impl Hash for LineText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        // Identical fixed chunk boundaries for flat and split forms, with slice
        // hashing rather than one hasher call per cell. Equal text hashes equally
        // for every Hasher, including when a public caller pushes a stored row.
        if let Some(prefix) = &self.prefix {
            u16::hash_slice(prefix, state);
        }
        for chunk in self.tail.chunks(64) {
            u16::hash_slice(chunk, state);
        }
    }
}
