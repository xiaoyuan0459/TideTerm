#pragma once
#include "cpp/chrome/menu_style.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QActionGroup>

namespace chrome {
inline void bind_input_modes(MainWindow *win) {
    auto *local = find_menu_action(win->menuBar(), QStringLiteral("mode:local"));
    auto *remote = find_menu_action(win->menuBar(), QStringLiteral("mode:remote"));
    auto *group = new QActionGroup(win);
    group->addAction(local);
    group->addAction(remote);
    local->setShortcutContext(Qt::WidgetShortcut);
    auto *toggle = new QAction(win);
    toggle->setShortcut(QKeySequence(QStringLiteral("Ctrl+Return")));
    toggle->setAutoRepeat(false);
    win->addAction(toggle);
    QObject::connect(toggle, &QAction::triggered, win, [win] {
        if (auto *view = win->active_view()) {
            term_set_reading(view->tab(), !term_reading(view->tab()));
            view->sync_reading_mode();
            win->refresh_chrome();
        }
    });
}
}
