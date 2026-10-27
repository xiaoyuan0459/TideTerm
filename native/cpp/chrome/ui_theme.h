// 主题色缓存（自绘件用）——QSS 到不了的地方（delegate/tab 角标/状态点）取色出口。
//
// 单一信源仍是 Rust `theme_db`：C++ 只在 `apply_theme()` 时经 `prefs_theme_colors()`
// 拉一次 POD 缓存，任何 TU 都不得写死色值。
#pragma once

#include <QColor>

#include <cstdint>

namespace chrome {

struct UiColors {
    QColor accent;
    /// 主文字色（页头亮图标/地址等自绘件；与 QSS `{text}` 同源）。
    QColor text;
    QColor text_secondary;
    QColor text_muted;
    QColor text_disabled;
    QColor bg_hover;
    QColor bg_elevated;
    QColor border1;
    QColor status_running;
    QColor status_connected;
    QColor status_disconnected;
    QColor warn;
    QColor danger;
    /// 终端正文底/前景（引擎 base16[0]/[7]，与 `term_apply_palette` 同一信源）。
    QColor term_bg;
    QColor term_fg;
};

/// 当前主题色（首次调用时惰性拉取；换主题后由 refresh_ui_colors() 失效）。
const UiColors &ui_colors();

/// 重新从 prefs 域桥拉取（MainWindow::apply_theme 调用）。
void refresh_ui_colors();

/// term_status_kind 类型码 →（webview ConnectionStatus 名, 中文文案）。
/// 单一信源：标签角标 / 状态栏两处共用，避免各自维护映射表而漂移。
struct StatusKind {
    const char *name;
    const char *text;
};
StatusKind status_kind(uint8_t code);

/// 连接状态字符串 → 状态点色（webview 六态语义，见清单 §4.5）。
QColor status_color(const QString &status);

/// 标签「活动方块」取色：与状态点**不同源**——参考 `center-area.css:86-118` 里
/// `.conn-activity.disconnected/.error` 都走 `--error`（红），而 `.conn-status.disconnected`
/// 走 `--status-disconnected`（灰）。清单 §4.5 的「六态 × 四面」差异即此。
QColor activity_color(const QString &status);

} // namespace chrome
