#include "cpp/chrome/shell_panel.h"
#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/display_names.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/chrome/text_marks.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QDockWidget>
#include <QMenu>
#include <QAction>
#include <QSignalBlocker>
#include <QStandardPaths>
#include <QTabBar>
#include <QToolButton>

ShellPanel::ShellPanel(MainWindow *window) : QTabWidget(window), window_(window) {
    setObjectName(QStringLiteral("shellTabs"));
    setTabsClosable(true);
    setMovable(true);
    setDocumentMode(true);
    setMinimumSize(120, 150); // Keep several terminal rows visible after docking/tabifying.
    tabBar()->setFixedHeight(chrome::metrics::kTabBarHeight);
    auto *add = new QToolButton(this);
    chrome::set_button_icon(add, QStringLiteral("add"), 14);
    add->setToolTip(i18n::tr("新建 Shell"));
    add->setFixedSize(24, 24);
    auto *menu = new QMenu(add);
    chrome::style_menu(menu);
    const QStringList names = {QStringLiteral("pwsh"), QStringLiteral("powershell"),
        QStringLiteral("cmd"), QStringLiteral("bash"), QStringLiteral("zsh"), QStringLiteral("fish")};
    for (const QString &name : names) {
        if (QStandardPaths::findExecutable(name).isEmpty()) continue;
        auto *action = menu->addAction(chrome::scalable_icon(QStringLiteral("terminal"), 14), chrome::shell_menu_name(name));
        connect(action, &QAction::triggered, this, [this, name] { open_shell(name); });
    }
    add->setMenu(menu);
    add->setPopupMode(QToolButton::MenuButtonPopup);
    connect(add, &QToolButton::clicked, this, [this] { open_shell(QStringLiteral("auto")); });
    setCornerWidget(add);
    connect(this, &QTabWidget::tabCloseRequested, this, &ShellPanel::close_shell);
    connect(this, &QTabWidget::currentChanged, this, [this] {
        if (auto *view = active_view()) view->setFocus();
    });
}

ShellPanel::~ShellPanel() {
    for (auto *view : MainWindow::views(this)) {
        view->mark_closed(); // 封桥后再终止，防止析构排队期间的事件再发 term_*
        term_close_tab(view->tab());
    }
}

void ShellPanel::ensure_shell() {
    if (count() == 0) open_shell(QStringLiteral("auto"));
}

void ShellPanel::open_shell(const QString &name) {
    const auto value = name.toUtf8();
    const uint32_t id = context_open_shell(rust::Str(value.constData(), size_t(value.size())));
    if (id == UINT32_MAX) return;
    term_set_render_opts(id, sess_active_config(id).options.cursor_style, true);
    auto *view = new PtyView(id, true);
    view->setMinimumHeight(100);
    // 会话图标按协议：SSH 显示远程主机（显示器+>_），本地 cmd/ps 用 terminal
    // ——两者必须可区分（WT 语义）；kind 见 bridge/session.rs（0=local 1=ssh…）。
    const uint8_t kind = sess_active_config(id).kind;
    const int index =
        addTab(view, chrome::scalable_icon(chrome::protocol_icon_for_kind(kind), 14),
               MainWindow::q_title(id));
    setCurrentIndex(index);
    view->setFocus();
}

bool ShellPanel::poll(bool titles) {
    bool visible_redraw = false;
    for (int index = 0; index < count(); ++index) {
        auto *view = dynamic_cast<PtyView *>(widget(index));
        if (!view) continue;
        const bool changed = view->poll_and_update(false); // Shared backend already drained.
        visible_redraw |= changed && view->isVisible();
        if (titles) {
            const QString title = MainWindow::q_title(view->tab());
            if (tabText(index) != title) setTabText(index, title);
        }
    }
    return visible_redraw;
}

void ShellPanel::close_shell(int index) {
    auto *view = dynamic_cast<PtyView *>(widget(index));
    if (!view || !chrome::prepare_mark_close(window_, {view})) return;
    {
        QSignalBlocker block(this);
        removeTab(index);
    }
    view->mark_closed(); // 封桥后再终止，防止删除前排队事件再发 term_*
    term_close_tab(view->tab());
    delete view;
    if (count() == 0) {
        if (auto *dock = qobject_cast<QDockWidget *>(parentWidget())) dock->hide();
    } else if (auto *next = active_view()) next->setFocus();
    window_->refresh_chrome();
}

bool ShellPanel::focus_session(uint32_t id) {
    for (int index = 0; index < count(); ++index) {
        auto *view = dynamic_cast<PtyView *>(widget(index));
        if (!view || view->tab() != id) continue;
        if (auto *dock = qobject_cast<QDockWidget *>(parentWidget())) { dock->show(); dock->raise(); }
        setCurrentIndex(index);
        view->setFocus();
        return true;
    }
    return false;
}

bool ShellPanel::owns_focus() const {
    auto *focused = QApplication::focusWidget();
    return focused && (focused == this || isAncestorOf(focused));
}

PtyView *ShellPanel::active_view() const {
    return dynamic_cast<PtyView *>(currentWidget());
}
