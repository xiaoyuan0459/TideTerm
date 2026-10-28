//! Shared path validation helpers.
//!
//! 复刻自旧 Tauri 后端 `src-tauri/src/path_util.rs`：把「绝对路径 + 禁止 `..`
//! 穿越」这一基线收敛到一处，供打开文件、SFTP 本地传输、日志导出等入口共用
//! （旧版评审 R1 指出各调用点此前在重复实现）。

use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Validate a filesystem path: must be absolute and must not contain
/// `..` components. Returns the parsed path on success.
pub fn validate_absolute_path(path: &str) -> Result<PathBuf> {
    let parsed = PathBuf::from(path);
    if !parsed.is_absolute() {
        return Err(Error::Config("path must be absolute".into()));
    }
    if path_contains_traversal(&parsed) {
        return Err(Error::Config("path must not contain `..` segments".into()));
    }
    Ok(parsed)
}

/// Return true when the path contains any `..` components.
pub fn path_contains_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_clean_absolute_paths() {
        // Windows vs Unix absolute-path syntax differs; assert per platform.
        if cfg!(target_os = "windows") {
            assert!(validate_absolute_path("C:\\Users\\file").is_ok());
            assert!(validate_absolute_path("C:\\usr\\local\\bin").is_ok());
        } else {
            assert!(validate_absolute_path("/home/user/file.txt").is_ok());
            assert!(validate_absolute_path("/usr/local/bin").is_ok());
        }
    }

    #[test]
    fn rejects_relative_paths() {
        let err = validate_absolute_path("relative/path.txt").unwrap_err();
        assert!(err.to_string().contains("must be absolute"));
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        assert!(validate_absolute_path("/home/../etc").is_err());
        assert!(validate_absolute_path("../etc").is_err());
        assert!(validate_absolute_path("/home/user/../../etc").is_err());
        assert!(validate_absolute_path("C:\\Users\\..\\Windows").is_err());
    }

    #[test]
    fn rejects_empty_path() {
        assert!(validate_absolute_path("").is_err());
    }

    #[test]
    fn traversal_detector_matches_components() {
        assert!(path_contains_traversal(Path::new("/a/../b")));
        assert!(!path_contains_traversal(Path::new("/a/b")));
    }
}
