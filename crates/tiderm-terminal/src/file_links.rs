//! Local file targets for the terminal's read-only hit testing. Opening belongs to the host.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct LocalTarget {
    pub abs_path: String,
}

pub(crate) fn resolve(cwd: Option<&str>, token: &str) -> Option<LocalTarget> {
    if token.is_empty() || token.len() > 4096 || token.contains(['\0', '\r', '\n']) {
        return None;
    }
    let home =
        || std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let candidate = if token == "~" {
        home()?
    } else if let Some(tail) = token
        .strip_prefix("~/")
        .or_else(|| token.strip_prefix("~\\"))
    {
        home()?.join(tail)
    } else {
        PathBuf::from(token)
    };
    let path = if candidate.is_absolute() {
        candidate
    } else {
        cwd.filter(|base| Path::new(base).is_absolute())
            .map(PathBuf::from)
            .or_else(home)?
            .join(candidate)
    };
    #[cfg(windows)]
    if matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..) | std::path::Prefix::DeviceNS(..)))
    {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    let metadata = canonical.metadata().ok()?;
    if !metadata.is_file() && !metadata.is_dir() {
        return None;
    }
    let text = canonical.to_str()?;
    #[cfg(windows)]
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    Some(LocalTarget {
        abs_path: text.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_targets_use_the_current_directory_and_require_real_files() {
        let directory = std::env::temp_dir().join(format!("tideterm-paths-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("a")).unwrap();
        std::fs::create_dir_all(directory.join("b")).unwrap();
        std::fs::write(directory.join("a/中文 note.txt"), b"fixture").unwrap();
        let a = directory.join("a");
        let b = directory.join("b");
        let target = resolve(a.to_str(), "./中文 note.txt").unwrap();
        assert!(Path::new(&target.abs_path).is_absolute());
        assert_eq!(std::fs::read(&target.abs_path).unwrap(), b"fixture");
        assert!(resolve(b.to_str(), "./中文 note.txt").is_none());
        assert!(resolve(a.to_str(), "missing.txt").is_none());
        assert!(resolve(a.to_str(), "file\0name").is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
