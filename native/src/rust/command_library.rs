//! Explicit personal commands. History is never persisted here.
use crate::bridge::completion::completion_ffi::{CommandCatalog, UserCommand};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_BYTES: u64 = 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u32,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: u32,
    group: String,
    name: String,
    command: String,
    #[serde(default)]
    is_template: bool,
}
pub(super) struct Library {
    path: PathBuf,
    entries: Vec<UserCommand>,
    revision: u64,
    error: u8,
}
impl Library {
    fn open(path: PathBuf) -> Self {
        let (entries, error) = match load(&path) {
            Ok(entries) => (entries, 0),
            Err(error) => (Vec::new(), error),
        };
        Self {
            path,
            entries,
            revision: 1,
            error,
        }
    }
    fn save(&mut self, revision: u64, entries: &[UserCommand]) -> u8 {
        if self.error != 0 {
            return self.error;
        }
        if revision != self.revision {
            return 3;
        }
        if let Err(error) = save(&self.path, entries) {
            return error;
        }
        self.entries = entries.to_vec();
        self.revision += 1;
        0
    }
}
fn path() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
        Ok(ROOT
            .get_or_init(|| tempfile::tempdir().expect("command test directory"))
            .path()
            .join("user-commands.json"))
    }
    #[cfg(not(test))]
    {
        tiderm_core::config_dir().map(|dir| dir.join("user-commands.json"))
    }
}
fn library() -> &'static Mutex<Library> {
    static LIBRARY: OnceLock<Mutex<Library>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        Mutex::new(match path() {
            Ok(path) => Library::open(path),
            Err(_) => Library {
                path: PathBuf::new(),
                entries: vec![],
                revision: 1,
                error: 1,
            },
        })
    })
}
pub fn command_catalog() -> CommandCatalog {
    let library = library().lock().unwrap_or_else(|e| e.into_inner());
    if super::term_ops::input_locked() {
        return CommandCatalog {
            revision: library.revision,
            entries: vec![],
            error: 5,
        };
    }
    CommandCatalog {
        revision: library.revision,
        entries: library.entries.clone(),
        error: library.error,
    }
}
pub fn command_catalog_save(revision: u64, entries: &[UserCommand]) -> u8 {
    if super::term_ops::input_locked() {
        return 5;
    }
    library()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .save(revision, entries)
}
pub(super) fn revision() -> u64 {
    library().lock().unwrap_or_else(|e| e.into_inner()).revision
}
fn valid(entries: &[UserCommand]) -> bool {
    let mut ids = std::collections::HashSet::new();
    entries.len() <= 200
        && entries.iter().all(|e| {
            e.id != 0
                && ids.insert(e.id)
                && !e.name.trim().is_empty()
                && e.name.len() <= 128
                && e.group.len() <= 128
                && !e.command.trim().is_empty()
                && e.command.len() <= 4096
                && (!e.is_template
                    || super::command_template::command_template_fields(&e.command).error == 0)
                && [&e.name, &e.group, &e.command]
                    .iter()
                    .all(|s| !s.chars().any(char::is_control))
        })
}
fn load(path: &Path) -> Result<Vec<UserCommand>, u8> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(1),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| 1)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(1);
    }
    let profile: Profile = serde_json::from_slice(&bytes).map_err(|_| 1)?;
    if !matches!(profile.version, 1 | 2)
        || (profile.version == 1 && profile.entries.iter().any(|e| e.is_template))
    {
        return Err(1);
    }
    let entries: Vec<_> = profile
        .entries
        .into_iter()
        .map(|e| UserCommand {
            id: e.id,
            group: e.group,
            name: e.name,
            command: e.command,
            is_template: e.is_template,
        })
        .collect();
    if !valid(&entries) {
        return Err(1);
    }
    Ok(entries)
}
fn save(path: &Path, entries: &[UserCommand]) -> Result<(), u8> {
    if !valid(entries) {
        return Err(2);
    }
    let entries = entries
        .iter()
        .map(|e| Entry {
            id: e.id,
            group: e.group.clone(),
            name: e.name.clone(),
            command: e.command.clone(),
            is_template: e.is_template,
        })
        .collect();
    let bytes = serde_json::to_vec_pretty(&Profile {
        version: 2,
        entries,
    })
    .map_err(|_| 2)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(2);
    }
    let parent = path.parent().ok_or(4)?;
    std::fs::create_dir_all(parent).map_err(|_| 4)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| 4)?;
    file.write_all(&bytes).map_err(|_| 4)?;
    file.as_file().sync_all().map_err(|_| 4)?;
    file.persist(path).map_err(|_| 4)?;
    Ok(())
}

#[cfg(test)]
#[path = "command_library_tests.rs"]
mod tests;
