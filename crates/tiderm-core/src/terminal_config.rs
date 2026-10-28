//! Persisted terminal preferences (v0.4 P1).
//!
//! 复刻自旧 Tauri 后端 `src-tauri/src/terminal_config.rs`；字段用 `snake_case`
//! 与 TypeScript `TerminalConfig` 接口逐字段一致（AGENTS.md fullstack-integration 规则）。

use serde::{Deserialize, Serialize};

/// Terminal display preferences persisted to `AppConfig.terminal`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_font_size")]
    pub font_size: u16,
    /// Whether font ligatures are enabled (v0.8).
    #[serde(default = "default_font_ligatures")]
    pub font_ligatures: bool,
    #[serde(default)]
    pub auto_copy_on_select: bool,
    #[serde(default = "default_scrollback")]
    pub scrollback: u32,
    #[serde(default = "default_right_click_selects_word")]
    pub right_click_selects_word: bool,
}

fn default_font_size() -> u16 {
    13
}

fn default_font_ligatures() -> bool {
    true
}

fn default_scrollback() -> u32 {
    // 10K lines keeps per-terminal memory bounded under multi-session
    // usage (review P3-1); users can raise it via terminal settings.
    10_000
}

fn default_right_click_selects_word() -> bool {
    true
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_ligatures: default_font_ligatures(),
            auto_copy_on_select: false,
            scrollback: default_scrollback(),
            right_click_selects_word: default_right_click_selects_word(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = TerminalConfig::default();
        assert_eq!(cfg.font_size, 13);
        assert!(cfg.font_ligatures);
        assert!(!cfg.auto_copy_on_select);
        assert_eq!(cfg.scrollback, 10_000);
        assert!(cfg.right_click_selects_word);
    }

    #[test]
    fn serializes_to_snake_case() {
        let cfg = TerminalConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(json.contains("font_size"));
        assert!(json.contains("font_ligatures"));
        assert!(json.contains("auto_copy_on_select"));
        assert!(json.contains("right_click_selects_word"));
    }

    #[test]
    fn deserializes_with_defaults() {
        let json = "{}";
        let cfg: TerminalConfig = serde_json::from_str(json).expect("deserialize");
        assert_eq!(cfg.font_size, 13);
        assert_eq!(cfg.scrollback, 10_000);
    }

    #[test]
    fn partial_json_keeps_explicit_values() {
        let cfg: TerminalConfig =
            serde_json::from_str(r#"{"font_size": 15, "scrollback": 20000}"#).expect("parse");
        assert_eq!(cfg.font_size, 15);
        assert_eq!(cfg.scrollback, 20_000);
        assert!(cfg.font_ligatures, "未提供的字段回落默认值");
    }
}
