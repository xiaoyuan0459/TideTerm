//! Crash recovery — session state snapshot persistence (de-shelled from
//! `src-tauri/crash_recovery.rs`). The `#[tauri::command]` wrappers stay in the Tauri
//! app; these are the pure save / load / clear functions.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Metadata envelope around the opaque layout payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySnapshot {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// ISO-8601 timestamp of when the snapshot was taken.
    pub timestamp: String,
    /// Number of tabs at snapshot time (for UI display).
    pub tab_count: u32,
    /// The active session ID at snapshot time.
    pub active_session_id: Option<String>,
    /// Opaque JSON — the frontend's `PersistedSplitLayout` blob.
    pub layout: serde_json::Value,
}

impl RecoverySnapshot {
    /// A human-readable summary for the crash-recovery dialog.
    pub fn summary(&self) -> String {
        format!(
            "检测到 {} 个会话的崩溃快照（{}）",
            self.tab_count, self.timestamp
        )
    }
}

/// Return the path to the recovery snapshot file.
fn snapshot_path() -> Result<PathBuf, String> {
    let base = crate::config_dir()?;
    Ok(base.join("recovery.json"))
}

/// Persist a recovery snapshot to disk (write-then-rename for atomicity).
pub fn save_snapshot(snapshot: &RecoverySnapshot) -> Result<(), String> {
    let path = snapshot_path()?;
    let data =
        serde_json::to_string_pretty(snapshot).map_err(|e| format!("serialize snapshot: {e}"))?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data.as_bytes()).map_err(|e| format!("write temp snapshot: {e}"))?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename snapshot: {e}"));
    }
    Ok(())
}

/// Load a recovery snapshot if one exists (corrupted snapshots are discarded).
pub fn load_snapshot() -> Option<RecoverySnapshot> {
    let path = snapshot_path().ok()?;
    let data = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<RecoverySnapshot>(&data) {
        Ok(s) if s.version == 1 => Some(s),
        Ok(_) => None,
        Err(_) => {
            let _ = fs::remove_file(&path);
            None
        }
    }
}

/// Remove the recovery snapshot file (clean exit or user rejected recovery).
pub fn clear_snapshot() -> Result<(), String> {
    let path = snapshot_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("remove snapshot: {e}"))?;
    }
    Ok(())
}

/// Build a snapshot with the current timestamp (used by the Tauri command wrapper).
pub fn build_snapshot(
    tab_count: u32,
    active_session_id: Option<String>,
    layout: serde_json::Value,
) -> RecoverySnapshot {
    RecoverySnapshot {
        version: 1,
        timestamp: Utc::now().to_rfc3339(),
        tab_count,
        active_session_id,
        layout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());

        let snap = build_snapshot(2, Some("pane-0".into()), serde_json::json!({"a": 1}));
        assert_eq!(snap.version, 1);
        assert!(snap.summary().contains("2"));

        save_snapshot(&snap).expect("save");
        let loaded = load_snapshot().expect("load");
        assert_eq!(loaded.tab_count, 2);
        assert_eq!(loaded.active_session_id.as_deref(), Some("pane-0"));

        clear_snapshot().expect("clear");
        assert!(load_snapshot().is_none());
    }

    #[test]
    fn missing_snapshot_returns_none() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());
        assert!(load_snapshot().is_none());
    }
}
