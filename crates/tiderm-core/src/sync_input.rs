//! Synchronized-input group state (de-shelled from `src-tauri/commands/sync_input.rs`).
//!
//! The de-shelled API exposes the group state + broadcast write; the persistent group
//! lives in the owning session manager. This module re-exports the event payload type
//! and a pure state helper so both the Tauri app and native shell model it identically.

use serde::{Deserialize, Serialize};

/// Event payload emitted when the sync-input group changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInputStateEvent {
    /// The session IDs currently in the sync group (empty = no sync).
    pub ids: Vec<String>,
}

/// A pure view of the sync-input group state.
#[derive(Debug, Clone, Default)]
pub struct SyncInputState {
    /// The session IDs currently in the sync group (empty = no sync).
    pub ids: Vec<String>,
}

impl SyncInputState {
    /// Whether the sync group is active.
    pub fn is_active(&self) -> bool {
        !self.ids.is_empty()
    }

    /// Human-readable summary for the panel.
    pub fn summary(&self) -> String {
        if self.is_active() {
            format!("已同步 {} 个会话", self.ids.len())
        } else {
            "未启用同步输入".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_inactive() {
        let s = SyncInputState::default();
        assert!(!s.is_active());
        assert_eq!(s.summary(), "未启用同步输入");
    }

    #[test]
    fn nonempty_state_is_active() {
        let s = SyncInputState {
            ids: vec!["a".into(), "b".into()],
        };
        assert!(s.is_active());
        assert!(s.summary().contains("2"));
    }
}
