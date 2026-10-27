//! Downloads commit without overwriting; failures delete only their temporary file.
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub(super) struct Download {
    file: tempfile::NamedTempFile,
    destination: PathBuf,
    pub size: Option<u64>,
    pub written: u64,
}
impl Download {
    pub fn new(path: &Path, size: Option<u64>) -> Result<Self, String> {
        if path.exists() {
            return Err("download destination already exists".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            destination: path.into(),
            size,
            written: 0,
        })
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let count = self.size.map_or(bytes.len(), |size| {
            bytes.len().min(size.saturating_sub(self.written) as usize)
        });
        self.file
            .write_all(&bytes[..count])
            .map_err(|e| e.to_string())?;
        self.written += count as u64;
        Ok(count)
    }
    pub fn finish(self) -> Result<(), String> {
        if self.size.is_some_and(|size| size != self.written) {
            return Err("received file size does not match header".into());
        }
        self.file.as_file().sync_all().map_err(|e| e.to_string())?;
        self.file
            .persist_noclobber(self.destination)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
pub(super) fn safe_name(raw: &[u8]) -> Result<String, String> {
    let name = std::str::from_utf8(raw).map_err(|_| "file name is not UTF-8")?;
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
    {
        return Err("unsafe incoming file name".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("reserved incoming file name".into());
    }
    Ok(name.into())
}
