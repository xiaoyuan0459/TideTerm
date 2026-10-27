#pragma once
#include <QHeaderView>
#include <QTreeWidget>
// Directory priority and numeric/date sorting are view semantics.
class FileItem : public QTreeWidgetItem {
public:
    explicit FileItem(QTreeWidget *tree) : QTreeWidgetItem(tree) {}
    bool operator<(const QTreeWidgetItem &other) const override {
        const bool dir = data(0, Qt::UserRole + 1).toBool();
        const bool other_dir = other.data(0, Qt::UserRole + 1).toBool();
        if (dir != other_dir) {
            const bool ascending = !treeWidget() ||
                treeWidget()->header()->sortIndicatorOrder() == Qt::AscendingOrder;
            return ascending ? dir : !dir;
        }
        const int col = treeWidget() ? treeWidget()->sortColumn() : 0;
        if (col == 3) return data(0, Qt::UserRole + 2).toULongLong()
                             < other.data(0, Qt::UserRole + 2).toULongLong();
        return QString::compare(text(col), other.text(col), Qt::CaseInsensitive) < 0;
    }
};
