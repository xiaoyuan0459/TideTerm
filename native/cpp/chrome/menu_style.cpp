// menu_style 实现（WT 对齐批次 4）。无 Q_OBJECT/moc：动作管线用 QObject::connect + lambda。
#include "cpp/chrome/menu_style.h"

#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QIcon>
#include <QMenu>
#include <QMenuBar>

namespace chrome {

void style_menu(QMenu *menu) {
    if (menu == nullptr) {
        return;
    }
    // 行高/图标槽靠 QSS + 图标尺寸合成（QAction 没有 setIconSize——那是按钮的接口）；
    // 这里只做 QSS 表达不了的一件事：分隔符不折叠（WT 的分组线是固定分组，不是自动合并）。
    menu->setSeparatorsCollapsible(false);
}

QAction *find_menu_action(QMenuBar *bar, const QString &tid) {
    if (bar == nullptr || tid.isEmpty()) {
        return nullptr;
    }
    // 菜单栏 → 子菜单 → 动作，深度优先（model 只有三层，无需通用遍历器）。
    QList<QAction *> stack = bar->actions();
    while (!stack.isEmpty()) {
        QAction *a = stack.takeFirst();
        if (a == nullptr) {
            continue;
        }
        if (a->property("tid").toString() == tid) {
            return a;
        }
        if (a->menu() != nullptr) {
            stack += a->menu()->actions();
        }
    }
    return nullptr;
}

void trigger_tid(MainWindow *win, const char *tid) {
    const QByteArray want(tid);
    for (QAction *a : win->findChildren<QAction *>()) {
        if (a->property("tid").toByteArray() == want) {
            a->trigger();
            return;
        }
    }
    win->note(i18n::tr("菜单动作不可用：%1").arg(QString::fromUtf8(want)));
}

QAction *proxy_action(QMenu *menu, QAction *src, const QString &icon_name,
                      const QString &text) {
    if (menu == nullptr) {
        return nullptr;
    }
    auto *proxy = menu->addAction(text);
    proxy->setProperty("chromeIcon", icon_name);
    proxy->setProperty("chromeIconSize", metrics::kMenuIconCanvas);
    if (!icon_name.isEmpty()) proxy->setIcon(scalable_icon(icon_name, metrics::kMenuIconCanvas));
    else if (src) proxy->setIcon(src->icon());
    if (src == nullptr) {
        proxy->setEnabled(false); // 无对应能力：置灰，不做死动作
        return proxy;
    }
    proxy->setEnabled(src->isEnabled());
    proxy->setCheckable(src->isCheckable());
    proxy->setChecked(src->isChecked());
    proxy->setShortcut(src->shortcut());
    proxy->setShortcutContext(Qt::WidgetShortcut);
    QObject::connect(src, &QAction::changed, proxy, [src, proxy] {
        proxy->setEnabled(src->isEnabled());
        proxy->setChecked(src->isChecked());
    });
    QObject::connect(proxy, &QAction::triggered, src, &QAction::trigger);
    return proxy;
}

QAction *add_action(QMenu *menu, const QString &icon_name, const QString &text, bool enabled,
                    std::function<void()> on_trigger) {
    if (menu == nullptr) {
        return nullptr;
    }
    auto *a = menu->addAction(text);
    a->setProperty("chromeIcon", icon_name);
    a->setProperty("chromeIconSize", metrics::kMenuIconCanvas);
    if (!icon_name.isEmpty()) a->setIcon(scalable_icon(icon_name, metrics::kMenuIconCanvas));
    a->setEnabled(enabled);
    if (enabled && on_trigger) {
        QObject::connect(a, &QAction::triggered, a, on_trigger);
    }
    return a;
}

void add_sep(QMenu *menu) {
    if (menu != nullptr) {
        menu->addSeparator();
    }
}

} // namespace chrome
