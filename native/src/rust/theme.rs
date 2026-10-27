//! Qt 壳主题服务（Phase C ⑦）——ThemeSpec schema + 主题查找 + QSS 生成。
//!
//! 主题的纯数据 schema、生成数据 [`theme_db::THEMES`] 与偏好持久化
//! [`ui_prefs`]（原子写/损坏备份/legacy 迁移）。二者内部
//! `crate::theme::…` 由 crate 根挂载本模块满足
//! （main.rs：`#[path = "rust/theme.rs"] pub mod theme;`）。
//!
//! QSS 生成留在 Rust 侧（业务逻辑不上 C++ 铁律）；C++ 只消费 `prefs_qss()`
//! 字符串喂 `QApplication::setStyleSheet`。

// ──────────────────── 生成数据 + 偏好持久化（零改动复用） ────────────────────

#[path = "shared/theme_db/mod.rs"]
pub mod theme_db;

#[path = "shared/ui_prefs.rs"]
pub mod ui_prefs;

pub use ui_prefs::UiPrefs;

// ──────────────────── ThemeSpec schema ────────────────────

/// Compact shell palette + terminal base-16 colours for one built-in theme.
/// 颜色为 `#RRGGBB` 或 `#RRGGBBAA`（alpha-last，theme_data 发出格式）。
pub struct ThemeSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub bg: &'static str,
    pub mantle: &'static str,
    pub surface: &'static str,
    pub surface2: &'static str,
    pub border: &'static str,
    pub text: &'static str,
    pub subtext: &'static str,
    pub faint: &'static str,
    pub accent: &'static str,
    pub danger: &'static str,
    pub scrim: &'static str,
    // ---- Tide (extended per-theme tokens) ----
    pub bg_elevated: &'static str,
    pub bg_input: &'static str,
    pub bg_tab_active: &'static str,
    pub border1: &'static str,
    pub border3: &'static str,
    pub text_disabled: &'static str,
    pub text_on_accent: &'static str,
    pub overlay_strong: &'static str,
    pub shadow: &'static str,
    pub shadow_elevated: &'static str,
    pub accent_hover: &'static str,
    pub accent_soft: &'static str,
    pub ai: &'static str,
    pub ai_soft: &'static str,
    pub ok: &'static str,
    pub warn: &'static str,
    pub info: &'static str,
    pub status_running: &'static str,
    pub status_connected: &'static str,
    pub status_disconnected: &'static str,
    pub base16: [&'static str; 16],
}

/// The default theme id (must exist in [`theme_db::THEMES`]).
/// 默认 WindTerm 深蓝灰主题（对齐原版 WindTerm 观感）。
pub const DEFAULT_THEME: &str = "windterm";

/// All built-in themes (same order as `native/themes`).
pub fn all() -> &'static [ThemeSpec] {
    theme_db::THEMES
}

/// Look a theme up by id, falling back to the default.
pub fn by_id(id: &str) -> &'static ThemeSpec {
    theme_db::THEMES
        .iter()
        .find(|t| t.id == id)
        .unwrap_or_else(|| {
            theme_db::THEMES
                .iter()
                .find(|t| t.id == DEFAULT_THEME)
                .expect("default theme must exist")
        })
}

// ──────────────────── QSS 生成（拆到 theme_qss.rs，避 500 行硬限） ────────────────────

#[path = "theme_qss.rs"]
mod qss;

pub use qss::{css, mix_alpha, qss_for};

/// 当前偏好 → 全局 QSS（C++ 桥入口）。
pub fn qss_current() -> String {
    qss_for(&ui_prefs::UiPrefs::load())
}

// ──────────────────── prefs 域桥实现（bridge/prefs.rs 包装为 prefs_* 桥名） ────────────────────

/// 一条主题（id + 显示名；首选项下拉用）。
pub struct ThemeItem {
    pub id: String,
    pub name: String,
}

/// 读当前偏好（ui_prefs 缓存；首次读盘后走内存）。
pub fn prefs_load() -> UiPrefs {
    ui_prefs::UiPrefs::load()
}

/// 保存偏好（原子写 + 缓存刷新；theme id 未知会被 normalized 回默认）。
pub fn prefs_save(prefs: &UiPrefs) {
    if let Err(e) = prefs.save() {
        tracing::error!("prefs save failed: {e}");
    }
}

/// 全部内置主题清单（THEMES 顺序，首选项下拉用）。
pub fn themes() -> Vec<ThemeItem> {
    all()
        .iter()
        .map(|t| ThemeItem {
            id: t.id.to_string(),
            name: t.name.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_exists_and_lookup_falls_back() {
        assert_eq!(by_id(DEFAULT_THEME).id, DEFAULT_THEME);
        // 未知 id 回落默认（与 ui_prefs::normalized 同语义）。
        assert_eq!(by_id("no-such-theme").id, DEFAULT_THEME);
    }

    #[test]
    fn qss_embeds_theme_background_and_handles_alpha() {
        let prefs = UiPrefs {
            theme: DEFAULT_THEME.to_string(),
            ..UiPrefs::default()
        };
        let t = by_id(DEFAULT_THEME);
        let qss = qss_for(&prefs);
        // 背景 token 经 css() 出现在 QSS（8 位 alpha-last hex → rgba()）。
        assert!(qss.contains(&css(t.bg)), "背景 token 必须出现在 QSS");
        assert!(qss.contains("rgba("), "alpha 通道必须转 rgba() 形式");
        // 换主题 → QSS 换底色（Light 的 bg 与默认深色不同源）。
        let light = qss_for(&UiPrefs {
            theme: "light".to_string(),
            ..UiPrefs::default()
        });
        assert_ne!(qss, light, "不同主题必须产生不同 QSS");
    }

    #[test]
    fn ui_prefs_roundtrip_via_shared_impl() {
        // 复用 ui_prefs 的 hermetic 路径 API（不经全局 config_dir）。
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ui_prefs.json");
        let prefs = UiPrefs {
            theme: "light".to_string(),
            ..UiPrefs::default()
        };
        prefs.save_to(&path).expect("save");
        assert_eq!(UiPrefs::load_from(&path).theme, "light");
    }
}
