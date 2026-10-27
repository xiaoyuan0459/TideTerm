//! Owned, immutable temporary history. mmap is opt-in and never used by live scrollback.
use memmap2::Mmap;
use std::io::{Read, Seek, SeekFrom};
use tempfile::NamedTempFile;

pub(super) const PAGE_BYTES: usize = 64 << 10;
const MAP_THRESHOLD: u64 = 200 << 20;

pub(super) struct HistoryFile {
    // Drop the mapping before removing its backing file (also on Windows).
    map: Option<Mmap>,
    pub(super) file: NamedTempFile,
    pub(super) bytes: u64,
}

impl HistoryFile {
    pub(super) fn new(file: NamedTempFile, bytes: u64, use_map: bool) -> Result<Self, String> {
        let map = if use_map && bytes >= MAP_THRESHOLD {
            // SAFETY: the file was created privately by this job, is sealed after export,
            // and no path or writable handle escapes while mapped. Launching a pager
            // consumes this owner and unmaps it before exposing the file path.
            Some(unsafe { Mmap::map(file.as_file()) }.map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self { map, file, bytes })
    }
    pub(super) fn mapped(&self) -> bool {
        self.map.is_some()
    }
    pub(super) fn page(&mut self, index: u32) -> Result<String, String> {
        let start = u64::from(index) * PAGE_BYTES as u64;
        if start >= self.bytes {
            return Ok(String::new());
        }
        let size = (self.bytes - start).min((PAGE_BYTES + 4) as u64) as usize;
        let mut buffer;
        let slice = if let Some(map) = &self.map {
            &map[start as usize..start as usize + size]
        } else {
            buffer = vec![0; size];
            self.file
                .as_file_mut()
                .seek(SeekFrom::Start(start))
                .map_err(|e| e.to_string())?;
            self.file
                .as_file_mut()
                .read_exact(&mut buffer)
                .map_err(|e| e.to_string())?;
            &buffer
        };
        let continuation = |b: u8| b & 0xc0 == 0x80;
        let mut from = 0;
        while from < slice.len() && continuation(slice[from]) {
            from += 1;
        }
        let mut end = PAGE_BYTES.min(slice.len());
        while end < slice.len() && continuation(slice[end]) {
            end += 1;
        }
        String::from_utf8(slice[from..end].to_vec()).map_err(|e| e.to_string())
    }
    pub(super) fn into_file(mut self) -> NamedTempFile {
        self.map = None;
        self.file
    }
}
