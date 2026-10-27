#include "cpp/chrome/ui_theme.h"

#include "cpp/bridge_shim.h"

namespace chrome {
namespace {

QColor hex(const rust::String &s) {
    return QColor(QString::fromUtf8(s.data(), int(s.size())));
}

UiColors load_colors() {
    const ThemeColorsPod p = prefs_theme_colors();
    UiColors c;
    c.accent = hex(p.accent);
    c.text = hex(p.text);
    c.text_secondary = hex(p.text_secondary);
    c.text_muted = hex(p.text_muted);
    c.text_disabled = hex(p.text_disabled);
    c.bg_hover = hex(p.bg_hover);
    c.bg_elevated = hex(p.bg_elevated);
    c.border1 = hex(p.border1);
    c.status_running = hex(p.status_running);
    c.status_connected = hex(p.status_connected);
    c.status_disconnected = hex(p.status_disconnected);
    c.warn = hex(p.warn);
    c.danger = hex(p.danger);
    c.term_bg = hex(p.term_bg);
    c.term_fg = hex(p.term_fg);
    return c;
}

UiColors &cache() {
    static UiColors cached = load_colors();
    return cached;
}

} // namespace

namespace {
constexpr StatusKind kKinds[] = {
    {"connecting", "连接中"},
    {"connected", "已连接"},
    {"running", "运行中"},
    {"disconnected", "已断开"},
    {"error", "异常"},
    {"unknown", "未连接"},
};
} // namespace

StatusKind status_kind(uint8_t code) {
    return (code >= 1 && code <= 6) ? kKinds[code - 1] : kKinds[5];
}

const UiColors &ui_colors() { return cache(); }

void refresh_ui_colors() { cache() = load_colors(); }

QColor status_color(const QString &status) {
    const UiColors &c = cache();
    if (status == QLatin1String("connected")) {
        return c.status_connected;
    }
    if (status == QLatin1String("running")) {
        return c.status_running;
    }
    if (status == QLatin1String("connecting") || status == QLatin1String("reconnecting")) {
        return c.warn;
    }
    if (status == QLatin1String("error")) {
        return c.danger;
    }
    if (status == QLatin1String("disconnected")) {
        return c.status_disconnected;
    }
    return c.text_disabled;
}

QColor activity_color(const QString &status) {
    const UiColors &c = cache();
    if (status == QLatin1String("disconnected") || status == QLatin1String("error")) {
        return c.danger;
    }
    return status_color(status);
}

} // namespace chrome
