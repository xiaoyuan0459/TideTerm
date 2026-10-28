#include "cpp/chrome/tree_delegate.h"

#include <QPainter>

#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/icon_cache.h"

namespace chrome {

QSize TreeDelegate::sizeHint(const QStyleOptionViewItem &, const QModelIndex &) const {
    return QSize(0, metrics::kTreeRowHeight);
}

void TreeDelegate::paint(QPainter *p, const QStyleOptionViewItem &opt,
                         const QModelIndex &index) const {
    const UiColors &c = ui_colors();
    const bool selected = opt.state.testFlag(QStyle::State_Selected);
    QRect r = opt.rect.adjusted(4, 0, -4, 0); // .tree-row { margin: 0 4px }

    p->save();
    p->setRenderHint(QPainter::Antialiasing);
    if (selected) {
        // 2026-10-06 用户反馈左缘强调条丑：改为整行圆角灰底（对齐 WT 面板
        // 行高亮——无彩色条），选中文字提亮，层次靠背景不靠色条。
        p->setPen(Qt::NoPen);
        p->setBrush(c.bg_hover);
        p->drawRoundedRect(r.adjusted(1, 1, -1, -1), 3, 3);
        p->setBrush(Qt::NoBrush);
    }

    const int kind = index.data(PodTreeModel::RoleKind).toInt();
    int x = r.left() + 4; // 行内左距（WT 实测组行距面板边 ~22px：indent 12 + 此 4 + 面板边距）
    const int cy = r.center().y();

    const QString proto = index.data(PodTreeModel::RoleProtocol).toString();
    const QString status = index.data(PodTreeModel::RoleStatus).toString();
    const QString glyph = kind == 0 ? QStringLiteral("folder") : protocol_icon(proto);
    const QColor ink = kind == 0 ? c.warn : status_color(status);
    scalable_icon(glyph, 14, ink).paint(p, QRect(x, cy - 7, 14, 14));
    x += 19;

    const QFont label = opt.font;
    p->setFont(label);
    p->setPen(kind != 0 && (status == QLatin1String("error") ||
                           status == QLatin1String("disconnected"))
                  ? c.danger
                  : (selected ? c.text : c.text_secondary));
    const QString text = index.data(Qt::DisplayRole).toString();
    QFontMetrics fm(label);
    const int avail = r.right() - x - 4;

    p->drawText(QRect(x, r.top(), qMax(0, avail), r.height()), Qt::AlignVCenter,
                fm.elidedText(text, Qt::ElideRight, qMax(0, avail)));
    p->restore();
}

} // namespace chrome
