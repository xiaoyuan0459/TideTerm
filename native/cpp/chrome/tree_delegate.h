// 连接树行 delegate —— webview `.tree-row` 的 1:1 自绘。
//
// 参考度量：padding 5×8、行内 gap 5、圆角 3、字号 12（次级文字）、状态点 8×8
// 圆角 2（**方形**，非圆形）、协议子标签 10px muted（side-panels.css:227-321）。
// 选中/hover 背景 bg-hover，选中额外带左侧 2px 强调条（`.tree-row.active::before`）。
//
// 无 Q_OBJECT：delegate 只覆写虚函数，不需要信号槽。
#pragma once

#include <QStyledItemDelegate>

namespace chrome {

class TreeDelegate : public QStyledItemDelegate {
public:
    using QStyledItemDelegate::QStyledItemDelegate;

    QSize sizeHint(const QStyleOptionViewItem &opt,
                   const QModelIndex &index) const override;
    void paint(QPainter *p, const QStyleOptionViewItem &opt,
               const QModelIndex &index) const override;
};

} // namespace chrome
