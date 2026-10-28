//! Bounded, credential-free window recovery points. Qt owns only layout state.
use super::store::{session_store, SessionConfig};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tiderm_core::config::SessionConfig as StoredSession;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_PANES: usize = 128;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    version: u8,
    pub(super) layout: Layout,
    pub(super) sessions: Vec<StoredSession>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Layout {
    Split {
        axis: u8,
        sizes: Vec<u32>,
        children: Vec<Layout>,
    },
    Group {
        active: usize,
        #[serde(default)]
        selected: bool,
        pages: Vec<Layout>,
    },
    Page {
        active: usize,
        tree: Box<Layout>,
    },
    Pane {
        session: usize,
    },
}

impl Layout {
    fn validate(
        &self,
        domain: u8,
        depth: usize,
        seen: &mut [bool],
        groups: &mut usize,
    ) -> Result<(), String> {
        if depth > 16 {
            return Err("workspace layout exceeds depth budget".into());
        }
        match self {
            Self::Split {
                axis,
                sizes,
                children,
            } if domain != 1 => {
                if *axis > 1
                    || children.is_empty()
                    || children.len() > MAX_PANES
                    || sizes.len() != children.len()
                    || sizes.iter().any(|v| *v > 1_000_000)
                {
                    return Err("invalid workspace split".into());
                }
                for child in children {
                    child.validate(domain, depth + 1, seen, groups)?;
                }
            }
            Self::Group { active, pages, .. } if domain == 0 => {
                *groups += 1;
                if *groups > 26
                    || pages.is_empty()
                    || pages.len() > MAX_PANES
                    || *active >= pages.len()
                {
                    return Err("invalid workspace group".into());
                }
                for page in pages {
                    page.validate(1, depth + 1, seen, groups)?;
                }
            }
            Self::Page { active, tree } if domain == 1 => {
                let before = seen.iter().filter(|v| **v).count();
                tree.validate(2, depth + 1, seen, groups)?;
                if *active >= seen.iter().filter(|v| **v).count() - before {
                    return Err("invalid workspace active pane".into());
                }
            }
            Self::Pane { session } if domain == 2 => {
                let entry = seen
                    .get_mut(*session)
                    .ok_or("invalid workspace session reference")?;
                if *entry {
                    return Err("duplicate workspace session reference".into());
                }
                *entry = true;
            }
            _ => return Err("invalid workspace layout node".into()),
        }
        Ok(())
    }
}

fn sanitized(cfg: &SessionConfig) -> StoredSession {
    let mut value = session_store::to_core(cfg);
    value.id = cfg.id.clone();
    if let Some(local) = &mut value.local {
        local.env.clear();
    }
    // Serialization also drops SSH/password, key passphrase and proxy password.
    serde_json::from_slice(&serde_json::to_vec(&value).expect("session serialization"))
        .expect("session round trip")
}

impl Snapshot {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.sessions.is_empty() || self.sessions.len() > MAX_PANES {
            return Err("unsupported or oversized workspace".into());
        }
        let mut seen = vec![false; self.sessions.len()];
        self.layout.validate(0, 0, &mut seen, &mut 0)?;
        if seen.contains(&false) {
            return Err("unreferenced workspace session".into());
        }
        for cfg in &self.sessions {
            if !matches!(
                cfg.kind.as_str(),
                "local" | "ssh" | "telnet" | "serial" | "tcp"
            ) {
                return Err("unknown workspace protocol".into());
            }
            super::session_advanced::validate(&session_store::from_core(cfg))?;
        }
        Ok(())
    }

    pub(super) fn parse(data: &str) -> Result<Self, String> {
        if data.len() > MAX_BYTES {
            return Err("workspace exceeds byte budget".into());
        }
        let mut value: Self = serde_json::from_str(data).map_err(|_| "invalid workspace file")?;
        value.validate()?;
        // Never trust credential fields added by hand to a recovery file.
        value.sessions = value
            .sessions
            .iter()
            .map(|cfg| sanitized(&session_store::from_core(cfg)))
            .collect();
        Ok(value)
    }

    fn encode(&self) -> Result<String, String> {
        let data = serde_json::to_string(self).map_err(|e| e.to_string())?;
        if data.len() > MAX_BYTES {
            return Err("workspace exceeds byte budget".into());
        }
        Ok(data)
    }
}

fn directory() -> Result<PathBuf, String> {
    Ok(super::store::state_directory()?.join("workspaces"))
}

fn file_at(root: &Path, key: &str) -> Result<PathBuf, String> {
    if key.is_empty()
        || key.len() > 64
        || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid workspace key".into());
    }
    Ok(root.join(format!("{key}.json")))
}

fn read_at(root: &Path, key: &str) -> Result<String, String> {
    let file = std::fs::File::open(file_at(root, key)?).map_err(|e| e.to_string())?;
    let mut data = String::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_string(&mut data)
        .map_err(|e| e.to_string())?;
    Snapshot::parse(&data)?.encode()
}

fn write_at(root: &Path, key: &str, data: &str) -> Result<(), String> {
    let path = file_at(root, key)?;
    let data = Snapshot::parse(data)?.encode()?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    file.write_all(data.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

pub fn workspace_capture(layout: &str, ids: &[u32]) -> Result<String, String> {
    if ids.is_empty() || ids.len() > MAX_PANES || layout.len() > MAX_BYTES {
        return Err("empty or oversized workspace".into());
    }
    let sessions = super::term::TABS.with(|tabs| {
        let tabs = tabs.borrow();
        ids.iter()
            .map(|id| {
                let state = tabs.get(id).ok_or("workspace session closed")?;
                let mut cfg = state.config.clone();
                if let Some(local) = &mut cfg.local
                    && state.context.cwd_reported
                {
                    local.working_directory = state.context.cwd.clone();
                }
                Ok(sanitized(&cfg))
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    let value = Snapshot {
        version: 1,
        layout: serde_json::from_str(layout).map_err(|_| "invalid workspace layout")?,
        sessions,
    };
    value.validate()?;
    value.encode()
}

pub fn workspace_save(key: &str, data: &str) -> Result<(), String> {
    write_at(&directory()?, key, data)
}
pub fn workspace_load(key: &str) -> Result<String, String> {
    read_at(&directory()?, key)
}

pub fn workspace_list() -> Vec<String> {
    let Ok(root) = directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let key = path.file_stem()?.to_str()?.to_owned();
            if path.extension()?.to_str()? != "json" || file_at(&root, &key).is_err() {
                return None;
            }
            Some((entry.metadata().ok()?.modified().ok()?, key))
        })
        .take(256)
        .collect();
    files.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    files.into_iter().take(32).map(|(_, key)| key).collect()
}

pub fn workspace_layout(data: &str) -> Result<String, String> {
    serde_json::to_string(&Snapshot::parse(data)?.layout).map_err(|e| e.to_string())
}

pub fn workspace_secret_flags(data: &str, index: usize) -> Result<u8, String> {
    let snapshot = Snapshot::parse(data)?;
    let cfg = snapshot
        .sessions
        .get(index)
        .ok_or("invalid workspace session")?;
    let mut cfg = session_store::from_core(cfg);
    if matches!(
        super::vault::inject_saved_credentials(&mut cfg),
        super::vault::Inject::Locked
    ) {
        return Ok(128);
    }
    let mut flags = 0;
    if let Some(ssh) = &cfg.ssh {
        flags |= match ssh.auth {
            super::store::session_config::SshAuth::Password if ssh.password.is_empty() => 1,
            super::store::session_config::SshAuth::Key if ssh.passphrase.is_empty() => 2,
            _ => 0,
        };
    }
    if !cfg.meta.options.ssh.proxy.username.is_empty() {
        flags |= 4;
    }
    Ok(flags)
}

pub fn workspace_open(
    data: &str,
    index: usize,
    password: &str,
    passphrase: &str,
    proxy: &str,
) -> Result<u32, String> {
    if super::term_ops::input_locked() {
        return Err("workspace restore blocked while locked".into());
    }
    let snapshot = Snapshot::parse(data)?;
    let mut cfg = session_store::from_core(
        snapshot
            .sessions
            .get(index)
            .ok_or("invalid workspace session")?,
    );
    if matches!(
        super::vault::inject_saved_credentials(&mut cfg),
        super::vault::Inject::Locked
    ) {
        return Ok(crate::bridge::session::VAULT_LOCKED);
    }
    if let Some(ssh) = &mut cfg.ssh {
        if !password.is_empty() {
            ssh.password = password.into();
        }
        if !passphrase.is_empty() {
            ssh.passphrase = passphrase.into();
        }
    }
    cfg.meta.options.ssh.proxy.password = proxy.into();
    Ok(super::term_create::open_tab_with_config(&cfg))
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
