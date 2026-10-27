#include "cpp/chrome/panel_controls.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/i18n.h"
#include <QAction>
#include <QApplication>
#include <QDockWidget>
#include <QMainWindow>
#include <QMenu>
#include <QPointer>
#include <QShortcut>

namespace chrome {
QList<QDockWidget *> managed_panels(QMainWindow *window) {
    QList<QDockWidget *> result;
    for (const char *name : {"dockConn", "dockFiles", "dockHistory", "dockSend", "dockSess", "dockShell", "dockTransfer"}) {
        if (auto *dock = window->findChild<QDockWidget *>(QLatin1String(name))) result << dock;
    }
    return result;
}

void split_panel(QMainWindow *window, QDockWidget *source, QDockWidget *target, Qt::Orientation orientation) {
    if (!source || !target || source == target) return;
    if (source->isFloating()) { window->addDockWidget(Qt::RightDockWidgetArea, source); source->setFloating(false); }
    target->setFloating(false); target->show(); source->show();
    // Removing peers leaves Qt's tab-layout placeholder intact. Promote the
    // source back to a standalone dock before splitting, then restore peers.
    const auto peers = window->tabifiedDockWidgets(source);
    if (!peers.isEmpty()) {
        const auto area = window->dockWidgetArea(source);
        for (auto *peer : peers) window->removeDockWidget(peer);
        window->removeDockWidget(source);
        window->addDockWidget(area == Qt::NoDockWidgetArea ? Qt::RightDockWidgetArea : area, source);
        source->show();
    }
    window->removeDockWidget(target);
    target->show();
    window->splitDockWidget(source, target, orientation);
    for (auto *peer : peers) if (peer != target) {
        window->tabifyDockWidget(source, peer); peer->show();
    }
    window->resizeDocks({source, target}, {1, 1}, orientation);
    target->raise();
}

void fill_panel_menu(QMenu *menu, QDockWidget *source) {
    auto *window = qobject_cast<QMainWindow *>(source->parentWidget());
    if (!window) return;
    style_menu(menu);
    auto *split = menu->addMenu(scalable_icon(QStringLiteral("split"), 14), i18n::tr("拆分(&S)"));
    for (auto orientation : {Qt::Vertical, Qt::Horizontal}) {
        auto *direction = split->addMenu(orientation == Qt::Vertical ? i18n::tr("上下拆分") : i18n::tr("左右拆分"));
        style_menu(direction);
        for (auto *target : managed_panels(window)) if (target != source) {
            auto *action = direction->addAction(target->windowTitle());
            QObject::connect(action, &QAction::triggered, window, [window, source, target, orientation] {
                split_panel(window, source, target, orientation);
            });
        }
    }
    auto *floating = menu->addAction(scalable_icon(source->isFloating() ? QStringLiteral("dock") : QStringLiteral("float"), 14),
        source->isFloating() ? i18n::tr("停靠(&F)") : i18n::tr("浮动(&F)"));
    QObject::connect(floating, &QAction::triggered, source, [source] { source->setFloating(!source->isFloating()); source->show(); });
    menu->addSeparator();
    const QStringList labels = {i18n::tr("资源管理器(&E)"), i18n::tr("文件管理器(&F)"),
        i18n::tr("历史命令(&O)"), i18n::tr("发送(&S)"), i18n::tr("会话(&S)"), i18n::tr("Shell(&S)"), i18n::tr("传输(&T)")};
    const QStringList icons = {QStringLiteral("network"), QStringLiteral("folder"),
        QStringLiteral("history"), QStringLiteral("send"), QStringLiteral("sessions"),
        QStringLiteral("terminal"), QStringLiteral("transfer")};
    const auto panels = managed_panels(window);
    for (int i = 0; i < panels.size(); ++i) {
        auto *target = panels[i];
        auto *action = menu->addAction(scalable_icon(icons.value(i), 14), labels.value(i, target->windowTitle()));
        action->setCheckable(true); action->setChecked(target == source);
        QObject::connect(action, &QAction::triggered, window, [window, source, target] {
            if (target != source) {
                target->setFloating(false); target->show();
                window->tabifyDockWidget(source, target);
            }
            target->show(); target->raise();
            if (target->widget()) target->widget()->setFocus();
        });
    }
}

void install_panel_shortcuts(QMainWindow *window) {
    window->setDockOptions(QMainWindow::AllowNestedDocks | QMainWindow::AllowTabbedDocks);
    for (auto orientation : {Qt::Horizontal, Qt::Vertical}) {
        auto *shortcut = new QShortcut(QKeySequence(orientation == Qt::Horizontal ? "Ctrl+Alt+Shift+H" : "Ctrl+Alt+Shift+V"), window);
        shortcut->setContext(Qt::ApplicationShortcut); // Also works in floated panels.
        QObject::connect(shortcut, &QShortcut::activated, window, [window, orientation] {
            QDockWidget *source = nullptr;
            for (auto *widget = QApplication::focusWidget(); widget; widget = widget->parentWidget())
                if (auto *dock = qobject_cast<QDockWidget *>(widget)) { source = dock; break; }
            auto panels = managed_panels(window);
            if (!source) source = window->findChild<QDockWidget *>(QStringLiteral("dockSess"));
            const int at = panels.indexOf(source);
            if (at >= 0 && panels.size() > 1) split_panel(window, source, panels[(at + 1) % panels.size()], orientation);
        });
    }
}
}
