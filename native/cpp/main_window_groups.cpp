#include "cpp/main_window.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/terminal/terminal_view.h"
#include <QInputDialog>
#include <algorithm>
#include <QTabWidget>

int MainWindow::page_index(QWidget *root) const {
    const auto found = std::find(page_roots_.begin(), page_roots_.end(), root);
    return found == page_roots_.end() ? -1 : int(found - page_roots_.begin());
}

void MainWindow::activate_group(QTabWidget *group) {
    if (!workspace_->groups().contains(group)) return;
    tabs_ = group;
    sync_send_panel(); refresh_chrome();
}

void MainWindow::split_group(Qt::Orientation orientation, bool clone_session) {
    auto *source = tabs_;
    auto *view = active_view();
    if (!source || !view) return;
    const int span = orientation == Qt::Horizontal ? source->width() : source->height();
    const int minimum = orientation == Qt::Horizontal ? 240 : 140;
    if (span < minimum * 2 + 5) {
        note(i18n::tr("当前会话区域太小，请先放大窗口、收起侧栏或开启专注模式。"));
        return;
    }
    QWidget *page = source->currentWidget();
    // A single-tab group opens a matching new session. With several tabs,
    // detach the selected existing page; its process and history stay alive.
    const bool duplicate = clone_session || source->count() == 1;
    uint32_t id = UINT32_MAX;
    if (duplicate) {
        id = term_split_tab(view->tab());
        if (id == UINT32_MAX) { note(i18n::tr("串口不能重复打开")); return; }
    }
    auto *created = workspace_->split_group(source, orientation);
    if (!created) { if (duplicate) term_close_tab(id); return; }
    if (duplicate) { tabs_ = created; add_tab_with(id); }
    else workspace_->move_page(page, created);
    if (auto *active = active_view()) active->setFocus();
    refresh_chrome();
}

void MainWindow::merge_current_group() {
    const auto groups = workspace_->groups();
    QStringList names;
    QList<QTabWidget *> targets;
    for (auto *group : groups) if (group != tabs_) {
        names << workspace_->group_name(group); targets << group;
    }
    if (targets.isEmpty()) { note(i18n::tr("只有一个标签组")); return; }
    QTabWidget *target = targets.front();
    if (targets.size() > 1) {
        bool ok = false;
        const QString picked = QInputDialog::getItem(this, i18n::tr("合并到组"),
            i18n::tr("目标标签组"), names, 0, false, &ok);
        if (!ok) return;
        target = targets[names.indexOf(picked)];
    }
    workspace_->merge_group(tabs_, target);
    if (auto *active = active_view()) active->setFocus();
    refresh_chrome();
}

void MainWindow::close_current_group() {
    QList<QPointer<QWidget>> pages;
    for (int i = 0; i < tabs_->count(); ++i) pages << tabs_->widget(i);
    for (const auto &page : pages) {
        const int at = page_index(page);
        if (at < 0) continue;
        chrome::request_close_tab(this, at);
        if (page_index(page) >= 0) break; // A cancelled confirmation stops the group close.
    }
}

void MainWindow::sync_group_order(QTabWidget *group) {
    // Global page indices remain aligned with banners/senders. Reorder only
    // the positions belonging to this group, leaving other groups' positions intact.
    std::vector<int> positions;
    for (int i = 0; i < page_count(); ++i)
        if (group->indexOf(page_roots_[size_t(i)]) >= 0) positions.push_back(i);
    if (int(positions.size()) != group->count()) return;
    const auto roots = page_roots_;
    const auto banners = banners_;
    const auto senders = sendbars_;
    for (int i = 0; i < group->count(); ++i) {
        const auto found = std::find(roots.begin(), roots.end(), group->widget(i));
        const size_t old = size_t(found - roots.begin()), target = size_t(positions[size_t(i)]);
        page_roots_[target] = roots[old]; banners_[target] = banners[old]; sendbars_[target] = senders[old];
    }
    sync_send_panel();
}
