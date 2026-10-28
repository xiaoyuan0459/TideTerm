#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/panel_splits.h"
#include "cpp/terminal/terminal_view.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QInputDialog>
#include <QLineEdit>
#include <QMessageBox>
#include <QSplitter>
#include <QTabWidget>
#include <QUuid>
#include <QTimer>
#include <QLayout>
#include <QPointer>
#include <exception>
#include <stdexcept>
#include <QDateTime>

namespace {
rust::Str rs(const QByteArray &s) { return rust::Str(s.constData(), size_t(s.size())); }
QByteArray bytes(const rust::String &s) { return QByteArray(s.data(), int(s.size())); }
QJsonObject split_node(QSplitter *split, const std::function<QJsonObject(QWidget *)> &capture) {
    QJsonArray children, sizes;
    for (int i = 0; i < split->count(); ++i) children.append(capture(split->widget(i)));
    for (int size : split->sizes()) sizes.append(qMax(1, size));
    return {{"type", "split"}, {"axis", split->orientation() == Qt::Horizontal ? 0 : 1},
        {"sizes", sizes}, {"children", children}};
}
}

QSplitter *MainWindow::page_splitter(QWidget *root) const {
    if (root && root->layout()) for (int i = 0; i < root->layout()->count(); ++i)
        if (auto *split = dynamic_cast<QSplitter *>(root->layout()->itemAt(i)->widget())) return split;
    return nullptr;
}

QJsonObject MainWindow::capture_page(QWidget *root, std::vector<uint32_t> *ids) const {
    std::function<QJsonObject(QWidget *)> capture = [&](QWidget *widget) -> QJsonObject {
        if (auto *split = dynamic_cast<QSplitter *>(widget)) {
            auto node = split_node(split, capture);
            if (split == zoom_splitter_ && zoom_sizes_.size() == size_t(split->count())) {
                QJsonArray sizes; for (int size : zoom_sizes_) sizes.append(qMax(1, size)); node["sizes"] = sizes;
            }
            return node;
        }
        auto *view = dynamic_cast<PtyView *>(widget);
        const int index = int(ids->size());
        if (view && !view->is_closed()) ids->push_back(view->tab());
        return {{"type", "pane"}, {"session", index}};
    };
    const auto panes = views(root);
    int active = 0;
    for (size_t i = 0; i < panes.size(); ++i) if (panes[i] == active_panes_.value(root)) active = int(i);
    return {{"type", "page"}, {"active", active}, {"tree", capture(page_splitter(root))}};
}

QByteArray MainWindow::capture_workspace(QWidget *only_page) const {
    std::vector<uint32_t> ids;
    std::function<QJsonObject(QWidget *)> capture = [&](QWidget *widget) -> QJsonObject {
        if (auto *split = dynamic_cast<QSplitter *>(widget)) return split_node(split, capture);
        auto *group = dynamic_cast<QTabWidget *>(widget);
        QJsonArray pages;
        if (group) for (int i = 0; i < group->count(); ++i) pages.append(capture_page(group->widget(i), &ids));
        return {{"type", "group"}, {"active", group ? qMax(0, group->currentIndex()) : 0}, {"selected", group == tabs_}, {"pages", pages}};
    };
    QJsonObject layout;
    if (only_page) layout = {{"type", "group"}, {"active", 0}, {"selected", true}, {"pages", QJsonArray{capture_page(only_page, &ids)}}};
    else layout = capture(workspace_->root_splitter());
    const QByteArray json = QJsonDocument(layout).toJson(QJsonDocument::Compact);
    return bytes(workspace_capture(rs(json), rust::Slice<const uint32_t>(ids.data(), ids.size())));
}

void MainWindow::remember_closed_tab(QWidget *root) {
    try {
        auto snapshot = capture_workspace(root);
        if (closed_tabs_.size() == 20) closed_tabs_.erase(closed_tabs_.begin());
        closed_tabs_.push_back(snapshot);
    } catch (const std::exception &) { note(i18n::tr("无法保存已关闭标签的恢复信息")); }
}

bool MainWindow::save_workspace() {
    if (page_roots_.empty()) { note(i18n::tr("当前窗口没有可保存的会话")); return true; }
    try {
        if (workspace_key_.isEmpty()) workspace_key_ = QDateTime::currentDateTimeUtc().toString(QStringLiteral("yyyyMMdd-HHmmss-"))
            + QUuid::createUuid().toString(QUuid::WithoutBraces);
        const auto snapshot = capture_workspace();
        workspace_save(rs(workspace_key_.toUtf8()), rs(snapshot));
        note(i18n::tr("会话工作区已保存"));
        return true;
    } catch (const std::exception &) {
        QMessageBox::warning(this, i18n::tr("保存失败"), i18n::tr("无法保存会话工作区，请检查配置目录和可用磁盘空间。"));
        return false;
    }
}

QWidget *MainWindow::restore_page(const QJsonObject &page, const QByteArray &snapshot) {
    std::vector<uint32_t> opened;
    std::function<QWidget *(const QJsonObject &)> build = [&](const QJsonObject &node) -> QWidget * {
        if (node.value("type").toString() == QLatin1String("pane")) {
            const size_t index = size_t(node.value("session").toInt());
            uint8_t flags = workspace_secret_flags(rs(snapshot), index);
            if (flags & 128) {
                if (!chrome::ensure_vault_ready(this)) throw std::runtime_error("cancelled");
                flags = workspace_secret_flags(rs(snapshot), index);
            }
            QByteArray secrets[3];
            const char *labels[] = {"会话密码", "密钥口令（无口令可留空）", "代理密码"};
            for (int bit = 0; bit < 3; ++bit) if (flags & (1 << bit)) {
                bool ok = false;
                secrets[bit] = QInputDialog::getText(this, i18n::tr("恢复会话"), i18n::tr(labels[bit]),
                    QLineEdit::Password, QString(), &ok).toUtf8();
                if (!ok) throw std::runtime_error("cancelled");
            }
            auto id = workspace_open(rs(snapshot), index, rs(secrets[0]), rs(secrets[1]), rs(secrets[2]));
            if (id == chrome::kVaultLocked && chrome::ensure_vault_ready(this))
                id = workspace_open(rs(snapshot), index, rs(secrets[0]), rs(secrets[1]), rs(secrets[2]));
            for (auto &secret : secrets) secret.fill('\0');
            if (id >= chrome::kVaultLocked) throw std::runtime_error("session open failed");
            opened.push_back(id);
            term_set_render_opts(id, sess_active_config(id).options.cursor_style, true);
            return new PtyView(id, true);
        }
        auto *split = chrome::panel_splitter(node.value("axis").toInt() == 0 ? Qt::Horizontal : Qt::Vertical);
        chrome::install_panel_split(split);
        try {
            for (const auto &child : node.value("children").toArray()) split->addWidget(build(child.toObject()));
        } catch (...) { delete split; throw; }
        QList<int> sizes;
        for (const auto &size : node.value("sizes").toArray()) sizes << qMax(1, size.toInt());
        split->setSizes(sizes);
        QTimer::singleShot(0, split, [split, sizes] { split->setSizes(sizes); });
        return split;
    };
    QWidget *tree = nullptr;
    try {
        tree = build(page.value("tree").toObject());
        // Reuse the production page/sender/banner registration, then replace its temporary view.
        add_tab_with(opened.front());
        QWidget *root = page_roots_.back();
        auto *split = page_splitter(root);
        for (auto *view : views(split)) view->mark_closed();
        if (!dynamic_cast<QSplitter *>(tree)) {
            auto *wrapper = chrome::panel_splitter(Qt::Vertical); wrapper->addWidget(tree); tree = wrapper;
        }
        tree->setParent(root);
        delete root->layout()->replaceWidget(split, tree);
        split->hide(); split->setParent(nullptr); split->deleteLater();
        tree->show();
        root->setProperty("workspaceTitle", q_title(opened.front()));
        auto panes = views(root);
        const int active = page.value("active").toInt();
        active_panes_[root] = panes[size_t(active)];
        return root;
    } catch (...) {
        delete tree;
        for (uint32_t id : opened) term_close_tab(id);
        throw;
    }
}

bool MainWindow::restore_workspace(const QByteArray &snapshot) {
    if (page_count()) return false;
    try {
        const auto layout = QJsonDocument::fromJson(bytes(workspace_layout(rs(snapshot)))).object();
        // Build pages in the initial group, then move them into the restored topology.
        std::vector<QWidget *> pages;
        std::function<void(const QJsonObject &)> collect = [&](const QJsonObject &node) {
            if (node.value("type").toString() == QLatin1String("group")) {
                for (const auto &page : node.value("pages").toArray()) pages.push_back(restore_page(page.toObject(), snapshot));
            } else for (const auto &child : node.value("children").toArray()) collect(child.toObject());
        };
        collect(layout);
        for (auto *group : workspace_->groups()) while (group->count()) group->removeTab(0);
        size_t at = 0;
        workspace_->restore_tree(layout, [&](const QJsonObject &) { return pages.at(at++); });
        if (auto *view = active_view()) view->setFocus();
        refresh_chrome();
        return true;
    } catch (const std::exception &) {
        while (page_count()) close_tab(page_count() - 1);
        closed_tabs_.clear();
        note(i18n::tr("会话恢复已取消或失败，原快照保持不变"));
        return false;
    }
}

void MainWindow::choose_workspace() {
    QStringList keys;
    for (const auto &key : workspace_list()) keys << QString::fromUtf8(key.data(), int(key.size()));
    if (keys.isEmpty()) { note(i18n::tr("没有已保存的会话工作区")); return; }
    QStringList choices;
    for (int i = 0; i < keys.size(); ++i) choices << i18n::tr("工作区 %1 · %2（UTC）").arg(i + 1)
        .arg(QDateTime::fromString(keys[i].left(15), QStringLiteral("yyyyMMdd-HHmmss")).toString(QStringLiteral("yyyy-MM-dd HH:mm:ss")));
    bool ok = false;
    const QString selected = QInputDialog::getItem(this, i18n::tr("恢复会话工作区"),
        i18n::tr("选择工作区（最近保存的在前）"), choices, 0, false, &ok);
    if (!ok) return;
    const QString key = keys.value(choices.indexOf(selected));
    try {
        const auto snapshot = bytes(workspace_load(rs(key.toUtf8())));
        if (QMessageBox::question(this, i18n::tr("恢复会话工作区"),
            i18n::tr("将在新窗口重新打开会话和分屏。原进程与输出不会恢复。继续？"),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes) return;
        auto *window = new MainWindow(false);
        window->setAttribute(Qt::WA_DeleteOnClose);
        window->show();
        if (!window->restore_workspace(snapshot)) window->deleteLater();
    } catch (const std::exception &) { note(i18n::tr("无法读取会话工作区，文件保持不变")); }
}

void MainWindow::reopen_closed_tab() {
    if (closed_tabs_.empty()) { note(i18n::tr("没有可重新打开的标签")); return; }
    const auto snapshot = closed_tabs_.back();
    try {
        const auto layout = QJsonDocument::fromJson(bytes(workspace_layout(rs(snapshot)))).object();
        restore_page(layout.value("pages").toArray().first().toObject(), snapshot);
        closed_tabs_.pop_back();
        refresh_chrome();
    } catch (const std::exception &) { note(i18n::tr("标签恢复已取消或失败，仍可重试")); }
}

bool MainWindow::restore_previous_workspace() {
    if (page_count()) return false;
    try {
        const auto keys = workspace_list();
        if (keys.empty()) return false;
        const auto snapshot = bytes(workspace_load(keys[0]));
        if (QMessageBox::question(this, i18n::tr("恢复会话工作区"),
            i18n::tr("恢复最近保存的会话和分屏？这会重新启动本地进程或连接远端会话。"),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes) return false;
        return restore_workspace(snapshot);
    } catch (const std::exception &) { note(i18n::tr("无法读取会话工作区，文件保持不变")); return false; }
}
