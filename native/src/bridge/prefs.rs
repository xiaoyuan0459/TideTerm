//! prefs 域桥（ADR-014 按域拆桥第六域，Phase C ⑦）。
//!
//! 偏好读写 + 主题清单 + QSS 全文。`prefs_` 前缀跨桥全局唯一；只传 POD。
//! 实现归 `crate::theme`（rust/theme.rs，crate 根挂载——theme_db/ui_prefs
//! 复用件以 `crate::theme::…` 引用）；本文件按 ai.rs 同款模式包装桥名。

pub use crate::theme::{prefs_load, prefs_save, qss_current, themes};

#[cxx::bridge]
pub mod prefs_ffi {
    /// 偏好 POD（与 Slint 壳 ui_prefs.json 同源字段）。
    pub struct UiPrefsPod {
        /// 内置主题 id（见 theme_db::THEMES；未知 id 保存时回落默认）。
        pub theme: String,
        pub restore_last_session: bool,
        pub cursor_blink: bool,
        /// SSH 连接超时（秒）。
        pub ssh_connect_timeout: u64,
        pub auto_accept_host_key: bool,
        /// 压缩回滚预算（MiB/窗格）。
        pub scrollback_mb: u64,
        /// 0=块 1=下划线 2=竖线（当前持久化 only——PtyView 渲染接入属 G1 域）。
        pub cursor_style: u8,
        pub line_numbers: bool,
        /// 界面语言：空 = 跟随系统；`zh_CN` / `en`。源语言为中文，其余走 i18n/.ts。
        pub locale: String,
    }

    /// 一条主题（id + 显示名）。
    pub struct ThemePod {
        pub id: String,
        pub name: String,
    }

    /// 自绘件需要的主题色（`#RRGGBB[AA]` 原样下发，C++ 侧 QColor 解析）。
    ///
    /// QSS 覆盖不到的地方（delegate 自绘、tab 角标、状态点）必须走这里取色，
    /// 禁止在 C++ 里写死色值——否则换主题时 chrome 会分叉。
    pub struct ThemeColorsPod {
        pub accent: String,
        /// 主文字色（页头亮图标/地址等自绘件用；QSS 的 `{text}` 同源）。
        pub text: String,
        pub text_secondary: String,
        pub text_muted: String,
        pub text_disabled: String,
        pub bg_hover: String,
        pub bg_elevated: String,
        pub border1: String,
        pub status_running: String,
        pub status_connected: String,
        pub status_disconnected: String,
        pub warn: String,
        pub danger: String,
        /// 终端正文底色/前景（= base16[0]/base16[7]，引擎 `set_base16` 同约定）。
        /// PtyView 在引擎 surface 之外的空白行需自绘同一底色，否则换主题时露旧色。
        pub term_bg: String,
        pub term_fg: String,
    }

    extern "Rust" {
        /// 读当前偏好（ui_prefs 缓存；读盘一次后走内存）。
        fn prefs_get() -> UiPrefsPod;
        /// 保存偏好（原子写 + rename；未知主题 id 自动回落默认）。
        fn prefs_set(prefs: &UiPrefsPod);
        /// 全部内置主题（THEMES 顺序）。
        fn prefs_themes() -> Vec<ThemePod>;
        /// 当前主题 QSS 全文（setStyleSheet 直接消费）。
        fn prefs_qss() -> String;
        /// 当前主题色（自绘件消费；apply_theme 时刷新缓存）。
        fn prefs_theme_colors() -> ThemeColorsPod;
        /// Optional screen-lock password; independent of the credential vault.
        fn prefs_lock_password_is_set() -> bool;
        /// Empty next keeps the password; removing requires old-password verification.
        fn prefs_lock_settings_save(
            current: String,
            next: String,
            removing: bool,
            minutes: u32,
        ) -> String;
        fn prefs_lock_password_verify(password: String) -> String;
        fn prefs_lock_idle_minutes() -> u32;
        fn prefs_lock_activity();
        fn prefs_lock_idle_due() -> bool;
    }
}

use crate::theme::ui_prefs::UiPrefs;
use prefs_ffi::{ThemeColorsPod, ThemePod, UiPrefsPod};

fn prefs_lock_password_is_set() -> bool {
    crate::rust::screen_lock::is_set()
}

fn prefs_lock_settings_save(current: String, next: String, removing: bool, minutes: u32) -> String {
    crate::rust::screen_lock::save(current, next, removing, minutes)
        .err()
        .unwrap_or_default()
}

fn prefs_lock_idle_minutes() -> u32 {
    crate::rust::screen_lock::idle_minutes()
}
fn prefs_lock_activity() {
    crate::rust::screen_lock::activity();
}
fn prefs_lock_idle_due() -> bool {
    crate::rust::screen_lock::idle_due()
}

fn prefs_lock_password_verify(password: String) -> String {
    crate::rust::screen_lock::verify(password)
        .err()
        .unwrap_or_default()
}

/// theme_db 的 alpha-last（`#RRGGBBAA`）→ Qt 的 alpha-first（`#AARRGGBB`）。
///
/// `QColor` 对 8 位 hex 只认 alpha 在前，直接下发会让 RGB 整体错位一个字节
/// （例：`#4a9effff` 会被读成 `rgb(0x9e,0xff,0xff)`）。QSS 侧由 `theme::css()`
/// 转 rgba()，本 POD 侧必须走这里。
fn qt_hex(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    match h.len() {
        8 => format!("#{}{}", &h[6..8], &h[0..6]),
        6 => format!("#{h}"),
        _ => "#000000".to_string(),
    }
}

/// 桥名包装：当前主题的自绘件用色。
pub(crate) fn prefs_theme_colors() -> ThemeColorsPod {
    let t = crate::theme::by_id(&prefs_load().theme);
    ThemeColorsPod {
        accent: qt_hex(t.accent),
        text: qt_hex(t.text),
        text_secondary: qt_hex(t.subtext),
        text_muted: qt_hex(t.faint),
        text_disabled: qt_hex(t.text_disabled),
        bg_hover: qt_hex(t.surface),
        bg_elevated: qt_hex(t.bg_elevated),
        border1: qt_hex(t.border1),
        status_running: qt_hex(t.status_running),
        status_connected: qt_hex(t.status_connected),
        status_disconnected: qt_hex(t.status_disconnected),
        warn: qt_hex(t.warn),
        danger: qt_hex(t.danger),
        term_bg: qt_hex(t.base16[0]),
        term_fg: qt_hex(t.base16[7]),
    }
}

/// 桥名包装：UiPrefs → UiPrefsPod（字段一一对应，无额外状态）。
pub(crate) fn prefs_get() -> UiPrefsPod {
    let p = prefs_load();
    UiPrefsPod {
        theme: p.theme,
        restore_last_session: p.restore_last_session,
        cursor_blink: p.cursor_blink,
        ssh_connect_timeout: p.ssh_connect_timeout,
        auto_accept_host_key: p.auto_accept_host_key,
        scrollback_mb: p.scrollback_mb,
        cursor_style: p.cursor_style,
        line_numbers: p.line_numbers,
        locale: p.locale,
    }
}

/// 桥名包装：UiPrefsPod → UiPrefs → 保存。
/// 主题 id 在写入口归一（ui_prefs::save 不做 normalized——Slint 壳靠 UI 下拉
/// 只产合法 id；Qt 壳在桥层兜底，脏 id 落盘前回落默认）。
pub(crate) fn prefs_set(prefs: &UiPrefsPod) {
    let mut ui = UiPrefs {
        theme: prefs.theme.clone(),
        restore_last_session: prefs.restore_last_session,
        cursor_blink: prefs.cursor_blink,
        ssh_connect_timeout: prefs.ssh_connect_timeout,
        auto_accept_host_key: prefs.auto_accept_host_key,
        scrollback_mb: prefs.scrollback_mb,
        cursor_style: prefs.cursor_style,
        line_numbers: prefs.line_numbers,
        locale: prefs.locale.clone(),
    };
    if crate::theme::by_id(&ui.theme).id != ui.theme {
        ui.theme = crate::theme::DEFAULT_THEME.to_string();
    }
    // 脏 locale 同样在此回落，与 ui_prefs::normalized 同语义。
    if ui.locale != "zh_CN" && ui.locale != "en" {
        ui.locale = String::new();
    }
    prefs_save(&ui);
}

/// 桥名包装：ThemeItem → ThemePod。
pub(crate) fn prefs_themes() -> Vec<ThemePod> {
    themes()
        .into_iter()
        .map(|t| ThemePod {
            id: t.id,
            name: t.name,
        })
        .collect()
}

/// 桥名包装：当前主题 QSS 全文。
pub(crate) fn prefs_qss() -> String {
    qss_current()
}

#[cfg(test)]
mod tests {
    use super::qt_hex;

    #[test]
    fn alpha_last_hex_becomes_qt_alpha_first() {
        // theme_db 全量 alpha-last；QColor 的 8 位格式是 alpha-first。
        assert_eq!(qt_hex("#4a9effff"), "#ff4a9eff");
        assert_eq!(qt_hex("#4a9eff24"), "#244a9eff");
        assert_eq!(qt_hex("#1a1f29"), "#1a1f29");
        assert_eq!(qt_hex("garbage"), "#000000");
    }

    #[test]
    fn light_theme_term_colors_follow_palette_convention() {
        // 引擎约定 base16[0]=默认背景、[7]=默认前景（set_base16 文档）。
        // light 必须给白底深字，否则终端出现黑底黑字（Q-05 复现形态）。
        let t = crate::theme::by_id("light");
        assert_eq!(qt_hex(t.base16[0]), "#ffffffff", "light 默认背景应为白");
        assert_eq!(qt_hex(t.base16[7]), "#ff1a1a1e", "light 默认前景应为深");
    }
}
