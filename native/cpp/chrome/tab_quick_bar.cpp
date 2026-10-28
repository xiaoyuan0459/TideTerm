// tab_quick_bar 实现（WT 对齐 2026-09-25）。无 Q_OBJECT/moc：QToolButton + lambda。

#include "cpp/chrome/tab_quick_bar.h"

#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/display_names.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QApplication>
#include <QFontMetrics>
#include <QHBoxLayout>
#include <QIcon>
#include <QMenu>
#include <QMenuBar>
#include <QTabWidget>
#include <QTabBar>
#include <QTimer>
#include <QToolButton>
#include <QStandardPaths>

namespace chrome {
namespace {

class TabQuickBar final : public QWidget {
public:
    explicit TabQuickBar(QTabWidget *tabs) : QWidget(tabs), tabs_(tabs) {
        tabs_->tabBar()->installEventFilter(this);
        connect(tabs_, &QTabWidget::currentChanged, this, [this] { align_later(); });
    }
    QSize sizeHint() const override {
        QSize size = QWidget::sizeHint();
        size.setHeight(metrics::kTabBarHeight);
        return size;
    }
    QSize minimumSizeHint() const override { return sizeHint(); }
protected:
    bool event(QEvent *event) override {
        const bool handled = QWidget::event(event);
        if (event->type() == QEvent::LayoutRequest || event->type() == QEvent::Resize ||
            event->type() == QEvent::Show) align_later();
        return handled;
    }
    bool eventFilter(QObject *object, QEvent *event) override {
        if (event->type() == QEvent::LayoutRequest || event->type() == QEvent::Resize ||
            event->type() == QEvent::FontChange || event->type() == QEvent::StyleChange)
            align_later();
        return QWidget::eventFilter(object, event);
    }
private:
    void align_later() {
        if (pending_) return;
        pending_ = true;
        QTimer::singleShot(0, this, [this] {
            pending_ = false;
            if (layout()) layout()->activate();
            auto *bar = tabs_->tabBar();
            auto *lead = bar->tabButton(bar->currentIndex(), QTabBar::LeftSide);
            if (!lead) return;
            const int y = lead->mapTo(tabs_, QPoint()).y() - mapTo(tabs_, QPoint()).y();
            for (auto *button : findChildren<QToolButton *>(QString(), Qt::FindDirectChildrenOnly))
                button->move(button->x(), qBound(0, y + (lead->height() - button->height()) / 2,
                                                height() - button->height()));
        });
    }
    QTabWidget *tabs_;
    bool pending_ = false;
};

/// 18×18 扁平钮（hover 底色由 QSS `#tabQuickBtn` 给）。
QToolButton *quick_btn(const QString &glyph, const QString &tip, QWidget *parent) {
    auto *b = new QToolButton(parent);
    b->setObjectName(QStringLiteral("tabQuickBtn"));
    b->setText(glyph);
    b->setToolTip(tip);
    b->setAccessibleName(tip);
    b->setAutoRaise(true);
    b->setFocusPolicy(Qt::NoFocus);
    b->setFixedSize(metrics::kTabQuickButtonSize, metrics::kTabQuickButtonSize);
    return b;
}

/// 同尺寸但用矢量图标（描边色随主题，见 icon_cache）。
QToolButton *quick_icon_btn(const QString &name, const QString &tip, QWidget *parent) {
    auto *b = quick_btn(QString(), tip, parent);
    const int g = metrics::kTabQuickButtonSize - 5; // 18 → 13，与菜单 14 字形同档
    set_button_icon(b, name, g);
    b->setIconSize(QSize(g, g));
    return b;
}

/// 菜单项 → 触发菜单栏里同一个 QAction（tid）。
QAction *tid_item(QMenu *menu, const QString &icon_name, const QString &text, MainWindow *win,
                  const char *tid) {
    return add_action(menu, icon_name, text, true, [win, tid] { trigger_tid(win, tid); });
}

/// 分组菜单项：文本与可用态都取自菜单栏里的**同一个**动作（表里 `merge-to-group` /
/// `close-group` 是 kDis ⇒ 自动置灰），触发即 `src->trigger()` ⇒ 行为仍走
/// `menu_bar.cpp`/`menu_actions.cpp` 的 dispatch 单点，本模块不复制拆分逻辑。
/// 表里查不到该 id 时**不放这一项**（不造假控件）。
void group_item(QMenu *menu, MainWindow *win, const char *tid, const QString &icon_name) {
    QAction *src = find_menu_action(win->menuBar(), QString::fromLatin1(tid));
    if (src == nullptr) {
        return;
    }
    proxy_action(menu, src, icon_name, src->text());
}

/// 标签页标题可能是一整条路径（本机 shell 的 OSC 标题），按像素宽中部省略，
/// 否则菜单会被撑到屏幕宽。
QString short_title(const QString &title) {
    return QFontMetrics(QApplication::font()).elidedText(title, Qt::ElideMiddle, 320);
}

/// 重填 `≡` 菜单（每次弹出都填：页数/标题都会变，静态快照会指错页）。
///
/// 标签页列表**平铺**在本菜单里，不做子菜单：实机验证——静态子菜单（`A ▸ 省略方式`）
/// 能正常弹出，而内容动态重建的子菜单（clear + 重填）在悬停/右方向键下都不弹
/// （Qt 判空后不再 popup 的路径吃掉了它）。平铺既绕开该行为，也少一层嵌套。
void fill_more_menu(QMenu *menu, MainWindow *win) {
    menu->clear();
    tid_item(menu, QStringLiteral("add"), i18n::tr("新建会话…"), win, "session:new");
    tid_item(menu, QStringLiteral("link"), i18n::tr("快速连接…"), win, "panel:quick");
    tid_item(menu, QStringLiteral("open"), i18n::tr("打开会话…"), win, "session:open");
    tid_item(menu, QStringLiteral("reopen"), i18n::tr("最近打开"), win, "session:recent");
    add_sep(menu);
    add_action(menu, QStringLiteral("terminal"), i18n::tr("本机终端"), true,
               [win] { win->add_tab(); });
    add_sep(menu);
    const int pages = win->page_count();
    if (pages == 0) {
        add_action(menu, QString(), i18n::tr("无标签"), false, nullptr);
    } else {
        const int current = win->current_page();
        for (int i = 0; i < pages; ++i) {
            QAction *a = add_action(
                menu, QStringLiteral("terminal"),
                QStringLiteral("%1. %2").arg(i + 1).arg(short_title(
                    QString::fromUtf8(win->tab_title(i).c_str()))),
                true, [win, i] { win->focus_session(win->page_id(i)); });
            a->setCheckable(true);
            a->setChecked(i == current);
        }
    }
    add_sep(menu);
    add_action(menu, QStringLiteral("close"), i18n::tr("关闭当前标签"), true,
               [win] { win->close_current_tab(); });
    add_action(menu, QStringLiteral("close"), i18n::tr("关闭其他标签"), true, [win] {
        const int keep = win->current_page();
        for (int i = win->page_count() - 1; i >= 0; --i) {
            if (i != keep) {
                win->close_tab(i);
            }
        }
    });
}

} // namespace

QWidget *build_tab_quick_bar(MainWindow *win, QTabWidget *tabs) {
    auto *bar = new TabQuickBar(tabs);
    bar->setObjectName(QStringLiteral("tabQuickBar"));
    bar->setFixedHeight(metrics::kTabBarHeight);
    auto *lay = new QHBoxLayout(bar);
    lay->setContentsMargins(0, 0, 6, 0);
    lay->setSpacing(1);
    lay->setAlignment(Qt::AlignVCenter);

    // ---- A：拆分组（WT 该钮的字母 = 分组编号，点开是分组菜单）----
    // 抓图证据：用户 2026-09-26 提供 WT 实拍——`A + ≡` 的 A 点开是
    // 水平拆分组(H) / 垂直拆分组(V) / 合并到分组(M) / 关闭分组；另一个分组显示 `C`。
    QToolButton *group = quick_btn(tabs->property("groupName").toString(), i18n::tr("拆分组"), bar);
    group->setProperty("groupHandle", true);
    auto *group_menu = new QMenu(group);
    style_menu(group_menu);
    QObject::connect(group_menu, &QMenu::aboutToShow, group_menu, [win, tabs, group_menu] {
        win->activate_group(tabs);
        group_menu->clear();
        group_item(group_menu, win, "window:split-group-horizontal", QStringLiteral("split_h"));
        group_item(group_menu, win, "window:split-group-vertical", QStringLiteral("split_v"));
        add_sep(group_menu);
        group_item(group_menu, win, "window:merge-to-group", QString());
        add_sep(group_menu);
        group_item(group_menu, win, "window:close-group", QString());
    });
    group->setMenu(group_menu);
    group->setPopupMode(QToolButton::DelayedPopup);
    QObject::connect(group, &QToolButton::clicked, group, [group] { group->showMenu(); });
    lay->addWidget(group);

    // ---- +：新建标签（本机 shell）----
    QToolButton *plus = quick_icon_btn(QStringLiteral("add"), i18n::tr("新建终端标签"), bar);
    auto *new_menu = new QMenu(plus);
    style_menu(new_menu);
    fill_new_session_menu(new_menu, win, [win, tabs] { win->activate_group(tabs); });
    plus->setMenu(new_menu);
    plus->setPopupMode(QToolButton::InstantPopup);
    lay->addWidget(plus);

    // ---- ≡：标签页列表 + 连接动作 ----
    QToolButton *more = quick_icon_btn(QStringLiteral("menu"), i18n::tr("标签页与连接"), bar);
    auto *more_menu = new QMenu(more);
    style_menu(more_menu);
    QObject::connect(more_menu, &QMenu::aboutToShow, more_menu,
                     [win, tabs, more_menu] { win->activate_group(tabs); fill_more_menu(more_menu, win); });
    fill_more_menu(more_menu, win); // 首次装配即填（空菜单不弹）
    more->setMenu(more_menu);
    more->setPopupMode(QToolButton::InstantPopup);
    lay->addWidget(more);

    return bar;
}

void fill_new_session_menu(QMenu *menu, MainWindow *win, const std::function<void()> &activate) {
    // WT 页头 □⁺▾ 实拍（2026-10-01）：本机终端用终端窗格图标，四个协议项全是链环 ∞。
    add_action(menu, QStringLiteral("terminal"), i18n::tr("本机终端"), true,
               [win, activate] { activate(); win->add_tab(); });
    auto *shells = menu->addMenu(i18n::tr("Shell"));
    style_menu(shells);
    for (const QString &shell : {QStringLiteral("powershell"), QStringLiteral("cmd"),
         QStringLiteral("bash"), QStringLiteral("zsh"), QStringLiteral("fish")}) {
        if (QStandardPaths::findExecutable(shell).isEmpty()) continue;
        add_action(shells, shell_icon(shell), shell_display_name(shell), true, [win, activate, shell] {
            activate();
            const uint32_t id = context_open_shell(rust::String(shell.toStdString()));
            if (id != UINT32_MAX) win->add_tab_with(id);
        });
    }
    add_sep(menu);
    const QStringList protocols = {QStringLiteral("SSH"), QStringLiteral("Telnet"),
                                   QStringLiteral("Serial"), QStringLiteral("TCP")};
    for (int i = 0; i < protocols.size(); ++i) {
        add_action(menu, protocol_icon(protocols[i]), protocols[i], true, [win, activate, i] {
            activate();
            SessCfg cfg = sess_form_get(rust::String(""), uint8_t(i + 1));
            bool connect_now = false;
            if (!run_session_form(win, &cfg, &connect_now)) return;
            if (connect_now) {
                const uint32_t id = sess_open_with_vault(win, cfg.id);
                if (id != UINT32_MAX && id != kVaultLocked) win->add_tab_with(id);
            }
            win->refresh_chrome();
        });
    }
}

} // namespace chrome
