//! Native UI preferences.
//!
//! Persisted to `{config_dir}/com.tideterm.app/ui_prefs.json` with the same
//! atomic write-then-rename + corrupted-file backup pattern used by
//! `tiderm_core::config::SessionStore`. Stores the selected theme, UI locale
//! and terminal/SSH defaults; chrome visibility is not persisted yet.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::theme;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// Built-in theme id (see `crate::theme`).
    pub theme: String,
    /// 常规: restore the last session on startup (Phase A: persisted only).
    #[serde(default)]
    pub restore_last_session: bool,
    /// 终端: blink the terminal cursor (Phase A: persisted only).
    #[serde(default = "default_true")]
    pub cursor_blink: bool,
    /// SSH: connect timeout in seconds (Phase A: persisted only).
    #[serde(default = "default_timeout")]
    pub ssh_connect_timeout: u64,
    /// SSH: auto-accept unknown host keys (Phase A: persisted only).
    #[serde(default)]
    pub auto_accept_host_key: bool,
    /// 终端: compressed scrollback budget in MiB per pane (P1.1; 32 = default cap).
    /// The store drops the oldest rows to stay within this budget; raise it for
    /// longer history at the cost of RAM (see `DEFAULT_COMPRESSED_CAP`).
    /// Consumed by `term_create::open_tab_with_config`; clamped to
    /// `SCROLLBACK_MB_MIN..=SCROLLBACK_MB_MAX` by [`UiPrefs::normalized`], which is also
    /// the range the preferences spin box offers (`cpp/chrome/prefs_dialog.cpp`).
    #[serde(default = "default_scrollback_mb")]
    pub scrollback_mb: u64,
    /// 终端: cursor shape (0 = 块 block, 1 = 下划线 underline, 2 = 竖线 bar)，
    /// 对齐 WindTerm 光标形状选项；渲染见 `RenderOpts::cursor_style`。
    #[serde(default)]
    pub cursor_style: u8,
    /// 终端: 行号 gutter（WindTerm 对齐；默认关）。渲染见 `RenderOpts::gutter`。
    #[serde(default)]
    pub line_numbers: bool,
    /// 界面语言：空串 = 跟随系统；否则为 BCP47 前缀（`zh_CN` / `en`）。
    /// 源语言为 zh_CN（代码里的文案即中文），其它语言由 `native/i18n/*.ts`
    /// 在运行时提供，缺失条目自动回落中文。
    #[serde(default)]
    pub locale: String,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    15
}

fn default_scrollback_mb() -> u64 {
    32
}

/// 每窗格压缩回滚预算的合法区间（MiB），与偏好页 spin box 的 setRange 同值。
pub const SCROLLBACK_MB_MIN: u64 = 4;
pub const SCROLLBACK_MB_MAX: u64 = 512;

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: theme::DEFAULT_THEME.to_string(),
            restore_last_session: false,
            cursor_blink: true,
            ssh_connect_timeout: 15,
            auto_accept_host_key: false,
            scrollback_mb: 32,
            cursor_style: 0,
            line_numbers: false,
            locale: String::new(),
        }
    }
}

impl UiPrefs {
    /// Process-wide cache so per-frame hot paths (the 16ms render pump drains events
    /// through `events::drain_events`) never hit the disk — perf audit G13.
    /// Repopulated by [`save`](Self::save); external edits to the file while the app
    /// runs are not a supported use case.
    fn cache() -> &'static std::sync::RwLock<Option<UiPrefs>> {
        static CACHE: std::sync::OnceLock<std::sync::RwLock<Option<UiPrefs>>> =
            std::sync::OnceLock::new();
        CACHE.get_or_init(|| std::sync::RwLock::new(None))
    }

    /// Default preference file (`<config>/com.tideterm.app/ui_prefs.json`).
    fn path() -> Result<PathBuf, String> {
        tiderm_core::config_dir().map(|dir| dir.join("ui_prefs.json"))
    }

    /// Load preferences from the default file, normalizing unknown theme ids.
    /// Served from the in-process cache after the first call.
    pub fn load() -> Self {
        if let Some(prefs) = Self::cache().read().unwrap().clone() {
            return prefs;
        }
        let path = Self::path().unwrap_or_else(|_| PathBuf::from("ui_prefs.json"));
        let prefs = Self::load_from(&path);
        *Self::cache().write().unwrap() = Some(prefs.clone());
        prefs
    }

    /// Persist preferences to the default file (atomic write + rename) and refresh
    /// the in-process cache.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        let result = self.save_to(&path);
        if result.is_ok() {
            *Self::cache().write().unwrap() = Some(self.clone());
        }
        result
    }

    /// Load preferences from an explicit path (hermetic tests).
    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            // Migration nicety: before ui_prefs.json existed, theme lived in the
            // Tauri config.json next to it. Preserve the user's old choice once.
            return Self::default_with_legacy_theme(path).normalized();
        }
        let data = match std::fs::read_to_string(path) {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("ui prefs read failed ({}): {e}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str::<UiPrefs>(&data) {
            Ok(prefs) => prefs.normalized(),
            Err(e) => {
                let backup = path.with_extension("json.bak");
                let _ = std::fs::copy(path, &backup);
                tracing::error!(
                    "ui prefs parse failed (backed up to {}): {e}",
                    backup.display()
                );
                Self::default()
            }
        }
    }

    /// Persist preferences to an explicit path (atomic write + rename).
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn default_with_legacy_theme(path: &Path) -> Self {
        let mut prefs = Self::default();
        if let Some(config) = path.parent().map(|parent| parent.join("config.json"))
            && let Ok(data) = std::fs::read_to_string(&config)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&data)
            && let Some(theme) = value.get("theme").and_then(|v| v.as_str())
        {
            prefs.theme = theme.to_string();
        }
        prefs
    }

    /// Replace an unknown theme id with the default so older versions can
    /// safely read a file written by a future version.
    pub(crate) fn normalized(mut self) -> Self {
        if theme::by_id(&self.theme).id != self.theme {
            self.theme = theme::DEFAULT_THEME.to_string();
        }
        // 合法取值只有「空 = 跟随系统」与已提供 .ts 的语言；其余回落跟随系统，
        // 避免把拼错的 locale 持久化成一个永远加载不到翻译的状态。
        if self.locale != "zh_CN" && self.locale != "en" {
            self.locale = String::new();
        }
        // 回滚预算即每窗格内存上限：手改 JSON 留下的 0 会裁光历史、超大值等于取消上限，
        // 在读盘这一层收口。区间与偏好页 spin box 的 setRange 同值。
        self.scrollback_mb = self
            .scrollback_mb
            .clamp(SCROLLBACK_MB_MIN, SCROLLBACK_MB_MAX);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_the_branded_dark_one() {
        assert_eq!(UiPrefs::default().theme, theme::DEFAULT_THEME);
    }

    #[test]
    fn unknown_locale_falls_back_to_system() {
        let norm = |locale: &str| {
            UiPrefs {
                locale: locale.to_string(),
                ..Default::default()
            }
            .normalized()
            .locale
        };
        assert_eq!(norm("klieg"), "");
        assert_eq!(norm("en"), "en");
        // 源语言显式选择也合法（等于关掉英文翻译）。
        assert_eq!(norm("zh_CN"), "zh_CN");
    }

    #[test]
    fn locale_round_trips_through_json() {
        let prefs = UiPrefs {
            locale: "en".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&prefs).expect("serialize");
        assert!(json.contains("\"locale\":\"en\""), "json was {json}");
        let back: UiPrefs = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.locale, "en");
        // 老文件没有 locale 字段时按「跟随系统」解析（serde default）。
        let legacy: UiPrefs =
            serde_json::from_str("{\"theme\":\"light\"}").expect("legacy deserialize");
        assert_eq!(legacy.locale, "");
    }

    #[test]
    fn migrates_theme_from_legacy_config_when_prefs_are_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("config.json"), r#"{ "theme": "light" }"#).expect("write");
        let path = dir.path().join("ui_prefs.json");
        assert_eq!(UiPrefs::load_from(&path).theme, "light");
        assert!(
            !path.exists(),
            "legacy migration must not create a prefs file"
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ui_prefs.json");
        let prefs = UiPrefs {
            theme: "light".to_string(),
            ..Default::default()
        };
        prefs.save_to(&path).expect("save");
        assert_eq!(UiPrefs::load_from(&path).theme, "light");
    }

    #[test]
    fn unknown_theme_normalizes_to_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ui_prefs.json");
        std::fs::write(&path, r#"{ "theme": "future-theme" }"#).expect("write");
        assert_eq!(UiPrefs::load_from(&path).theme, theme::DEFAULT_THEME);
    }

    /// 回滚预算就是每窗格的内存上限，`term_create` 把它直接当作压缩层的字节预算，
    /// 所以越界值必须在读盘这一层收口：0 会立即裁光历史，超大值等于取消上限。
    #[test]
    fn scrollback_mb_clamps_to_the_editor_range() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ui_prefs.json");
        for (raw, want) in [(0, SCROLLBACK_MB_MIN), (4096, SCROLLBACK_MB_MAX), (8, 8)] {
            std::fs::write(&path, format!("{{ \"scrollback_mb\": {raw} }}")).expect("write");
            assert_eq!(UiPrefs::load_from(&path).scrollback_mb, want, "raw {raw}");
        }
    }

    #[test]
    fn corrupted_file_is_backed_up_and_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ui_prefs.json");
        std::fs::write(&path, "{ not json").expect("write");
        let prefs = UiPrefs::load_from(&path);
        assert_eq!(prefs.theme, theme::DEFAULT_THEME);
        assert!(path.with_extension("json.bak").exists());
    }
}
