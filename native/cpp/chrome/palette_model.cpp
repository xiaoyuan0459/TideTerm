#include "cpp/chrome/palette_model.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QHash>
#include <QMenu>
#include <QMenuBar>
#include <QSet>
#include <functional>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
QString label(QString value) { return value.remove(QLatin1Char('&')).section(QLatin1Char('\t'), 0, 0); }
}
bool chrome::palette_action_ready(const PaletteItem &item) {
    if (!item.action || !item.action->isEnabled() || !item.action->isVisible() || item.action->property("tid").toByteArray() != item.tid) return false;
    for (const auto &parent : item.parents) if (!parent || !parent->isEnabled() || !parent->isVisible()) return false;
    return true;
}
bool chrome::palette_live_ready(MainWindow *window, const PaletteItem &item) {
    return window && item.view && !item.view->is_closed() && window->isAncestorOf(item.view) && term_history_state(item.view->tab()).available;
}
chrome::PaletteModel chrome::palette_model(MainWindow *window, PtyView *source, PaletteMode mode) {
    PaletteModel model;
    if (mode == PaletteMode::Actions) {
        QSet<QByteArray> seen;
        std::function<void(QMenu *, const QString &, QList<QPointer<QAction>>, int)> walk;
        walk = [&](QMenu *menu, const QString &path, QList<QPointer<QAction>> parents, int depth) {
            if (depth > 8) { model.truncated = true; return; }
            for (auto *a : menu->actions()) {
                if (!a->isVisible() || a->isSeparator()) continue;
                const QString title = path + QStringLiteral(" › ") + label(a->text());
                if (a->menu()) { auto next = parents; next << a; walk(a->menu(), title, next, depth + 1); continue; }
                const QByteArray tid = a->property("tid").toByteArray();
                if (tid.isEmpty() || tid == "view:command-palette" || seen.contains(tid)) continue;
                if (model.items.size() == 1024) { model.truncated = true; return; }
                seen.insert(tid); PaletteItem item; item.tid = tid; item.key = QString::fromUtf8(tid); item.title = title;
                item.action = a; item.parents = parents; item.shortcut = a->shortcut().toString(QKeySequence::NativeText);
                item.unavailable = a->toolTip();
                if (tid == "goto:position" || tid == "goto:symbol" || tid == "goto:session" || tid == "session:open") {
                    item.kind = PaletteKind::Mode;
                    item.mode = tid == "goto:position" ? PaletteMode::Lines : tid == "goto:symbol" ? PaletteMode::Symbols : PaletteMode::Sessions;
                }
                model.items << item;
            }
        };
        for (auto *top : window->menuBar()->actions()) if (top->menu()) walk(top->menu(), label(top->text()), {top}, 0);
    } else if (mode == PaletteMode::Symbols && source && !source->is_closed()) {
        for (const auto &r : term_commands(source->tab())) {
            PaletteItem item; item.kind = PaletteKind::Symbol; item.command = r.id; item.key = QStringLiteral("symbol:%1").arg(r.id);
            item.title = r.startup ? i18n::tr("连接启动输出") : q(r.title); item.detail = i18n::tr("行 %1").arg(r.line); model.items << item;
        }
    } else if (mode == PaletteMode::Sessions) {
        QHash<uint32_t, QPointer<PtyView>> owned; QHash<uint32_t, QString> locations; rust::Vec<uint32_t> ids;
        for (int page = 0; page < window->page_count(); ++page) {
            const auto panes = window->pane_ids(page);
            for (size_t pane = 0; pane < panes.size(); ++pane) locations.insert(panes[pane], i18n::tr("标签 %1 · 窗格 %2").arg(page + 1).arg(qulonglong(pane + 1)));
        }
        for (QWidget *widget : window->findChildren<QWidget *>()) {
            auto *view = dynamic_cast<PtyView *>(widget);
            if (!view || view->is_closed() || owned.contains(view->tab())) continue;
            if (ids.size() == 4096) { model.truncated = true; break; }
            owned.insert(view->tab(), view); ids.push_back(view->tab());
        }
        const auto snapshot = palette_sessions(rust::Slice<const uint32_t>(ids.data(), ids.size()));
        model.error = snapshot.error; model.truncated |= snapshot.truncated;
        for (const auto &r : snapshot.rows) {
            PaletteItem item; item.saved = q(r.saved); item.title = q(r.title);
            const bool live = r.tab != UINT32_MAX; item.kind = live ? PaletteKind::Live : PaletteKind::Saved;
            item.key = live ? QStringLiteral("live:%1").arg(r.tab) : QStringLiteral("saved:") + item.saved;
            item.view = owned.value(r.tab);
            item.detail = (live ? i18n::tr("已打开") : i18n::tr("打开会话")) + QStringLiteral(" · ") + q(r.protocol);
            if (live) item.detail += QStringLiteral(" · ") + locations.value(r.tab, i18n::tr("Shell 面板"));
            if (!r.group.empty()) item.detail += QStringLiteral(" · ") + q(r.group);
            model.items << item;
        }
    }
    return model;
}
