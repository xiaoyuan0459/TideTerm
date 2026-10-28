#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/ui_metrics.h"
#include <QAction>

namespace chrome {
void set_action_icon(QAction *action, const QString &id) {
    // A checkable menu row uses this slot for its state, rather than an icon.
    if (action->isCheckable()) {
        action->setProperty("chromeIcon", QString());
        action->setIcon(QIcon());
        return;
    }
    struct Mapping { const char *id; const char *icon; };
    static const Mapping mappings[] = {
        {"session:new", "pane_add"}, {"session:new-window", "terminal"},
        {"session:disconnect", "pane_close"}, {"session:reconnect", "pane_reconnect"},
        {"session:open", "open"}, {"session:recent", "reopen"},
        {"session:save", "save"}, {"session:save-selection", "download"},
        {"session:close-view", "close"},
        {"edit:copy", "copy"}, {"edit:paste", "paste"}, {"edit:select-all", "select_all"},
        {"edit:clear-screen", "screen_clear"}, {"edit:clear-scrollback", "clear_scroll"},
        {"mode:lock-screen", "lock"}, {"mode:lock-menu", "lock"},
        {"mode:lock-settings", "settings"}, {"mode:hex-edit", "binary"},
        {"window:split-view-horizontal", "split_h"}, {"window:split-view-vertical", "split_v"},
        {"window:split-group-horizontal", "split_h"}, {"window:split-group-vertical", "split_v"},
        {"window:close-pane", "pane_close"},
        {"window:zoom-pane", "zoom"}, {"window:merge-to-group", "dock"},
        {"panel:quick", "link"}, {"panel:sftp", "folder"}, {"panel:send", "send"},
        {"panel:log", "log"}, {"panel:history", "history"}, {"panel:sessions", "sessions"},
        {"panel:transfers", "transfer"}, {"panel:explorer", "network"},
        {"panel:shell", "terminal"}, {"help:about", "info"},
        {"tools:sync-input", "sync"}, {"tools:triggers", "trigger"},
        {"tools:transfer-binary", "transfer"},
        {"view:theme", "palette"}, {"view:zoom", "zoom_text"}, {"view:highlight-caret-line", "highlight"},
        {"mode:fullscreen", "fullscreen"}, {"mode:focus", "zoom"}, {"mode:focus-settings", "settings"},
        {"window:activate-next", "next"}, {"window:activate-previous", "prev"},
        {"view:command-palette", "command"},
        {"session:preferences", "settings"}, {"session:log", "log"},
        {"edit:paste-selected", "paste"}, {"edit:copy-session-url", "link"},
        {"view:toggle-menu-bar", "menu"}, {"view:fold", "fold"},
        {"search:find", "search"}, {"search:highlight", "highlight"},
        {"goto:previous-command", "prev"}, {"goto:next-command", "next"},
        {"pref:general", "settings"}, {"pref:terminal", "terminal"},
        {"ai:settings", "settings"},
    };
    QString name;
    for (const auto &mapping : mappings) if (id == QLatin1String(mapping.id)) { name = QString::fromLatin1(mapping.icon); break; }
    action->setProperty("chromeIcon", name);
    action->setProperty("chromeIconSize", metrics::kMenuIconCanvas);
    action->setIcon(name.isEmpty() ? QIcon() : scalable_icon(name, metrics::kMenuIconCanvas));
}
}
