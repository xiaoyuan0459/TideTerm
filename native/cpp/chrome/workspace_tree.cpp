#include "cpp/chrome/tab_workspace.h"
#include <QJsonArray>
#include <QSplitter>
#include <QTabWidget>
#include <QSignalBlocker>
#include <QTimer>
#include "cpp/chrome/split_ratio.h"

bool TabWorkspace::restore_tree(const QJsonObject &tree,
    const std::function<QWidget *(const QJsonObject &)> &page) {
    for (auto *group : groups_) if (group->count()) return false;
    for (auto *group : groups_) { group->hide(); group->setParent(nullptr); group->deleteLater(); }
    groups_.clear();
    while (root_->count()) { auto *child = root_->widget(0); child->hide(); child->setParent(nullptr); child->deleteLater(); }
    bool ok = true;
    QTabWidget *selected = nullptr;
    std::function<QWidget *(const QJsonObject &)> build = [&](const QJsonObject &node) -> QWidget * {
        if (node.value("type").toString() == QLatin1String("group")) {
            auto *group = create_group();
            if (node.value("selected").toBool()) selected = group;
            QSignalBlocker block(group);
            for (const auto &entry : node.value("pages").toArray()) {
                if (auto *widget = page(entry.toObject())) group->addTab(widget, widget->property("workspaceTitle").toString());
                else ok = false;
            }
            group->setCurrentIndex(node.value("active").toInt());
            return group;
        }
        auto *split = new QSplitter(node.value("axis").toInt() == 0 ? Qt::Horizontal : Qt::Vertical);
        chrome::install_split_ratio(split);
        for (const auto &child : node.value("children").toArray()) split->addWidget(build(child.toObject()));
        QList<int> sizes;
        for (const auto &size : node.value("sizes").toArray()) sizes << qMax(1, size.toInt());
        split->setSizes(sizes);
        QTimer::singleShot(0, split, [split, sizes] { split->setSizes(sizes); });
        return split;
    };
    root_->addWidget(build(tree));
    if (groups_.isEmpty()) root_->addWidget(create_group());
    activate(selected ? selected : first_group());
    if (on_changed) on_changed();
    return ok;
}
