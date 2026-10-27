#pragma once

#include <QString>
#include <QtGlobal>

class QPainter;
class QColor;

namespace chrome::detail {
void paint_icon_shape(QPainter &painter, const QString &name, qreal size, const QColor &ink, qreal dpr);
}
