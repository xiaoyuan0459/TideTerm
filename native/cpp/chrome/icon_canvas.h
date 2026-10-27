#pragma once

#include <QColor>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QString>
#include <cmath>
#include <initializer_list>

namespace chrome::detail {

// Shared 16-unit grid. Straight strokes land on physical pixels; curves retain AA.
// Geometry stays within 1..15 so even small action badges never clip.
class IconCanvas {
public:
    IconCanvas(QPainter &painter, qreal size, const QColor &ink, qreal dpr)
        : p(painter), ink(ink), scale_(size / 16), dpr_(dpr) {
        // 笔宽半物理像素步进（2026-10-06 用户反馈线粗）：1.25 缩放下 14px 图标
        // 得 1.5 物理宽（此前 1.4 系数 round 到 2，视觉比 WT 粗一档）；
        // 100% 缩放下回落 1.0，与 WT 菜单细线一致。相位按四舍五入后的奇偶。
        const qreal pixels = qMax(0.5, qRound(1.25 * scale_ * dpr * 2) / 2.0);
        const int rounded = qRound(pixels);
        phase_ = rounded % 2 ? 0.5 : 0;
        pen_ = QPen(ink, pixels / dpr, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin);
        p.setPen(pen_);
        p.setBrush(Qt::NoBrush);
    }
    QPointF at(qreal x, qreal y) const { return {snap(x), snap(y)}; }
    QRectF box(qreal x, qreal y, qreal w, qreal h) const {
        return QRectF(at(x, y), at(x + w, y + h));
    }
    void line(qreal x, qreal y, qreal x2, qreal y2) { p.drawLine(at(x, y), at(x2, y2)); }
    void poly(std::initializer_list<QPointF> points, bool closed = false, bool filled = false) {
        QPainterPath path;
        bool first = true;
        for (const auto &point : points) {
            if (first) path.moveTo(at(point.x(), point.y()));
            else path.lineTo(at(point.x(), point.y()));
            first = false;
        }
        if (closed) path.closeSubpath();
        p.setBrush(filled ? QBrush(ink) : QBrush(Qt::NoBrush));
        p.drawPath(path);
        p.setBrush(Qt::NoBrush);
    }
    void rect(qreal x, qreal y, qreal w, qreal h, bool filled = false) {
        // Filled edges lie on pixel boundaries, independent of the stroke's half-pixel phase.
        if (filled) p.fillRect(QRectF(edge(x, y), edge(x + w, y + h)), ink);
        else p.drawRect(box(x, y, w, h));
    }
    void round_rect(qreal x, qreal y, qreal w, qreal h) {
        p.drawRoundedRect(box(x, y, w, h), scale_, scale_);
    }
    void ellipse(qreal x, qreal y, qreal w, qreal h) { p.drawEllipse(box(x, y, w, h)); }
    void dot(qreal x, qreal y) {
        p.save();
        QPen dot_pen = pen_;
        dot_pen.setWidthF(qMax(2, qRound(1.8 * scale_ * dpr_)) / dpr_);
        p.setPen(dot_pen);
        p.drawPoint(at(x, y));
        p.restore();
    }
    void plus(qreal x, qreal y, qreal radius = 2) {
        line(x - radius, y, x + radius, y);
        line(x, y - radius, x, y + radius);
    }
    void cross(qreal x, qreal y, qreal radius = 2) {
        line(x - radius, y - radius, x + radius, y + radius);
        line(x + radius, y - radius, x - radius, y + radius);
    }
    void terminal(qreal x = 2, qreal y = 3, qreal w = 12, qreal h = 10) {
        round_rect(x, y, w, h);
        poly({{x + 2, y + 3}, {x + 4, y + 5}, {x + 2, y + 7}});
        line(x + 6, y + 7, x + 9, y + 7);
    }
    /// 远程主机终端（WT 语义：带底座的显示器 + >_）——与本地 terminal 的
    /// 纯窗口形区分：SSH 会话用这个，本地 cmd/ps/bash 用 terminal。
    void remote_host() {
        round_rect(2, 1, 12, 9);
        line(8, 10, 8, 12);
        line(5, 13, 11, 13);
        poly({{4, 4}, {6, 5.5}, {4, 7}});
        line(7, 7, 9, 7);
    }
    void folder() {
        poly({{1, 4}, {6, 4}, {8, 6}, {14, 6}, {14, 13}, {1, 13}}, true);
        line(1, 7, 14, 7);
    }
    void document() {
        poly({{3, 1}, {9, 1}, {13, 5}, {13, 14}, {3, 14}}, true);
        poly({{9, 1}, {9, 5}, {13, 5}});
    }
    QPainter &p;
    QColor ink;

private:
    qreal snap(qreal value) const {
        return (std::floor(value * scale_ * dpr_ - phase_ + 0.5) + phase_) / dpr_;
    }
    QPointF edge(qreal x, qreal y) const {
        return {qRound(x * scale_ * dpr_) / dpr_, qRound(y * scale_ * dpr_) / dpr_};
    }
    QPen pen_;
    qreal scale_;
    qreal dpr_;
    qreal phase_;
};

bool paint_action_icon(IconCanvas &c, const QString &name);
bool paint_file_icon(IconCanvas &c, const QString &name);

} // namespace chrome::detail
