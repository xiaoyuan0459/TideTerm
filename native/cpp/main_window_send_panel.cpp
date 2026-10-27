// Full-width bottom panel; each session keeps its own input and send settings.
#include "cpp/main_window.h"

#include <QDockWidget>
#include <QLineEdit>
#include <QStackedWidget>
#include <QTimer>
#include <QToolButton>

#include <algorithm>

#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/terminal/terminal_view.h"

std::vector<uint32_t> MainWindow::send_target_ids(uint32_t source, int scope) const {
    // Auxiliary shell views also own Rust sessions, without a page_roots_ entry.
    if (scope == 0) return {source};
    if (scope == 2) {
        std::vector<uint32_t> ids;
        for (auto *view : views(const_cast<MainWindow *>(this)))
            if (!view->is_closed()) ids.push_back(view->tab());
        return ids;
    }
    QWidget *source_root = nullptr;
    for (auto *root : page_roots_) for (auto *view : views(root))
        if (!view->is_closed() && view->tab() == source) source_root = root;
    if (!source_root || scope != 1) return {};
    std::vector<uint32_t> ids;
    auto *group = workspace_->owner(source_root);
    for (auto *root : page_roots_) {
        if (workspace_->owner(root) != group) continue;
        for (auto *view : views(root)) if (!view->is_closed()) ids.push_back(view->tab());
    }
    return ids;
}

void MainWindow::build_send_panel() {
    auto *dock = new QDockWidget(i18n::tr("发送"), this);
    dock->setObjectName(QStringLiteral("dockSend"));
    dock->setFeatures(QDockWidget::DockWidgetMovable | QDockWidget::DockWidgetClosable | QDockWidget::DockWidgetFloatable);
    chrome::install_panel_header(dock, i18n::tr("发送"))->set_dot(QStringLiteral("warn"));
    docks_->send = dock;
    send_stack_ = new QStackedWidget(dock);
    send_stack_->setObjectName(QStringLiteral("sendStack"));
    send_stack_->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    dock->setWidget(send_stack_);
    setCorner(Qt::BottomLeftCorner, Qt::BottomDockWidgetArea);
    setCorner(Qt::BottomRightCorner, Qt::BottomDockWidgetArea);
    addDockWidget(Qt::BottomDockWidgetArea, dock);
    resizeDocks({dock}, {chrome::metrics::kBottomPanelHeight}, Qt::Vertical);
    dock->hide();
}

void MainWindow::register_sendbar(SendBar *bar) {
    const bool first = send_stack_->count() == 0;
    send_stack_->addWidget(bar);
    send_stack_->setCurrentWidget(bar);
    auto *dock = qobject_cast<QDockWidget *>(send_stack_->parentWidget());
    if (!(property("focusMode").toBool() && property("focusHidesSend").toBool())) dock->show();
    if (first) {
        // Hidden, empty docks do not retain resizeDocks' requested size.
        QTimer::singleShot(0, dock, [this, dock] {
            if (!dock->isHidden()) {
                resizeDocks({dock}, {chrome::metrics::kBottomPanelHeight}, Qt::Vertical);
            }
        });
    }
}

void MainWindow::remove_sendbar(SendBar *bar) {
    // A queued timeout must not invoke a writer capturing the deleted page root.
    bar->set_writer_factory({}, {});
    bar->set_completion_source({});
    send_stack_->removeWidget(bar);
    bar->deleteLater();
    if (send_stack_->count() == 0) send_stack_->parentWidget()->hide();
}

void MainWindow::sync_send_panel() {
    const int i = current_page();
    if (i >= 0 && i < int(sendbars_.size())) {
        send_stack_->setCurrentWidget(sendbars_[size_t(i)]);
        sendbars_[size_t(i)]->sync_completion_source();
    }
}

void MainWindow::focus_send_input(QWidget *page_root) {
    const auto it = std::find(page_roots_.begin(), page_roots_.end(), page_root);
    if (it == page_roots_.end()) {
        return;
    }
    const int index = int(it - page_roots_.begin());
    if (index < 0 || index >= int(sendbars_.size())) {
        return;
    }
    if (auto *dock = qobject_cast<QDockWidget *>(send_stack_->parentWidget())) {
        dock->show();
    }
    if (tabs_ != nullptr && workspace_ != nullptr) {
        tabs_ = workspace_->owner(page_root);
        if (tabs_ != nullptr) tabs_->setCurrentWidget(page_root);
    }
    sync_send_panel();
    if (sendbars_[size_t(index)]->input()) {
        sendbars_[size_t(index)]->focus_input();
    }
}

void MainWindow::show_completion_sources(bool collect) {
    auto *view = active_view();
    if (!view || view->is_closed()) { note(i18n::tr("请先打开会话")); return; }
    const auto source = view->tab();
    for (auto *widget : findChildren<QWidget *>()) if (auto *bar = dynamic_cast<SendBar *>(widget); bar && bar->completion_source() == source) {
        if (send_stack_->indexOf(bar) >= 0) send_stack_->setCurrentWidget(bar);
        for (auto *parent = bar->parentWidget(); parent; parent = parent->parentWidget()) {
            if (auto *dock = dynamic_cast<QDockWidget *>(parent)) { dock->show(); dock->raise(); break; }
        }
        bar->show(); bar->focus_input();
        if (auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"))) {
            if (collect) button->click();
            else { button->setFocus(); note(i18n::tr("当前会话的路径与程序目录在发送栏启用；Shell 环境可显式采集命令名字和资源。")); }
        }
        return;
    }
    note(i18n::tr("当前会话没有可用的发送栏"));
}
