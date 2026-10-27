#include "cpp/chrome/icon_canvas.h"

namespace chrome::detail {

bool paint_action_icon(IconCanvas &c, const QString &name) {
    const auto n = [&](const char *value) { return name == QLatin1String(value); };
    if (n("copy")) {
        c.poly({{10, 3}, {10, 1}, {2, 1}, {2, 11}, {4, 11}});
        c.rect(5, 4, 8, 10);
        c.line(7, 7, 11, 7);
        c.line(7, 10, 11, 10);
    } else if (n("duplicate")) {
        c.poly({{11, 3}, {11, 1}, {1, 1}, {1, 11}, {3, 11}});
        c.rect(4, 4, 10, 10);
        c.line(4, 7, 14, 7);
        c.plus(9, 11, 2);
    } else if (n("paste")) {
        c.poly({{5, 3}, {3, 3}, {3, 14}, {13, 14}, {13, 3}, {11, 3}});
        c.round_rect(5, 1, 6, 4);
        c.line(6, 8, 10, 8);
        c.line(6, 11, 10, 11);
    } else if (n("command")) {
        c.poly({{2, 4}, {6, 8}, {2, 12}});
        c.line(8, 12, 14, 12);
    } else if (n("search") || n("zoom_text")) {
        c.ellipse(2, 2, 9, 9);
        c.line(10, 10, 14, 14);
        if (n("zoom_text")) c.plus(6, 6, 2);
    } else if (n("highlight")) {
        c.poly({{4, 9}, {10, 3}, {13, 6}, {7, 12}}, true);
        c.poly({{4, 9}, {2, 13}, {6, 12}}, true);
        c.line(8, 14, 14, 14);
    } else if (n("clear_scroll")) {
        for (int y : {3, 6, 9}) c.line(2, y, y == 9 ? 6 : 12, y);
        c.cross(11, 12, 2);
    } else if (n("screen_clear")) {
        c.poly({{13, 6}, {13, 2}, {2, 2}, {2, 11}, {6, 11}});
        c.line(4, 14, 7, 14);
        c.poly({{7, 11}, {11, 7}, {14, 10}, {10, 14}, {7, 14}}, true);
        c.line(9, 9, 12, 12);
    } else if (n("clear_input")) {
        c.poly({{1, 8}, {5, 3}, {14, 3}, {14, 13}, {5, 13}}, true);
        c.cross(10, 8, 2);
    } else if (n("select_all")) {
        c.poly({{2, 5}, {2, 2}, {5, 2}});
        c.poly({{11, 2}, {14, 2}, {14, 5}});
        c.poly({{14, 11}, {14, 14}, {11, 14}});
        c.poly({{5, 14}, {2, 14}, {2, 11}});
        c.line(5, 6, 11, 6);
        c.line(5, 10, 11, 10);
    } else if (n("prev") || n("next")) {
        const qreal edge = n("prev") ? 3 : 13;
        const qreal tip = n("prev") ? 5 : 11;
        const qreal tail = n("prev") ? 11 : 5;
        c.line(edge, 3, edge, 13);
        c.poly({{tail, 4}, {tip, 8}, {tail, 12}});
    } else if (n("refresh")) {
        QPainterPath circle;
        const QRectF ring = c.box(2, 2, 11, 11);
        circle.arcMoveTo(ring, 35);
        circle.arcTo(ring, 35, 290);
        c.p.drawPath(circle);
        c.poly({{14, 1}, {14, 6}, {9, 6}}, true, true);
    } else if (n("reset")) {
        QPainterPath circle;
        circle.moveTo(c.at(3, 6));
        circle.cubicTo(c.at(5, 0), c.at(14, 1), c.at(14, 8));
        circle.cubicTo(c.at(14, 14), c.at(5, 16), c.at(2, 10));
        c.p.drawPath(circle);
        c.poly({{3, 2}, {3, 6}, {7, 6}});
        c.dot(8, 8);
    } else if (n("sync")) {
        c.rect(1, 3, 5, 10);
        c.rect(10, 3, 5, 10);
        c.line(4, 6, 12, 6);
        c.poly({{10, 4}, {12, 6}, {10, 8}});
        c.line(12, 10, 4, 10);
        c.poly({{6, 8}, {4, 10}, {6, 12}});
    } else if (n("trigger")) {
        c.poly({{9, 1}, {3, 9}, {7, 9}, {6, 15}, {13, 6}, {9, 6}}, true);
    } else if (n("binary")) {
        c.rect(2, 4, 5, 8);
        c.poly({{10, 6}, {12, 4}, {12, 12}});
        c.line(10, 12, 14, 12);
    } else if (n("log")) {
        c.document();
        c.line(5, 8, 10, 8);
        c.line(5, 11, 10, 11);
    } else if (n("delete")) {
        c.poly({{4, 5}, {5, 14}, {11, 14}, {12, 5}});
        c.line(2, 4, 14, 4);
        c.poly({{6, 4}, {6, 2}, {10, 2}, {10, 4}});
        c.line(7, 7, 7, 11);
        c.line(9, 7, 9, 11);
    } else if (n("rename")) {
        c.poly({{2, 10}, {10, 2}, {14, 6}, {6, 14}, {2, 14}}, true);
        c.line(8, 4, 12, 8);
        c.line(2, 10, 6, 14);
    } else if (n("palette")) {
        QPainterPath path;
        path.moveTo(c.at(14, 9));
        path.cubicTo(c.at(16, 1), c.at(2, 0), c.at(2, 8));
        path.cubicTo(c.at(2, 14), c.at(10, 16), c.at(10, 11));
        path.cubicTo(c.at(10, 9), c.at(13, 11), c.at(14, 9));
        c.p.drawPath(path);
        c.dot(6, 5); c.dot(10, 5); c.dot(5, 9);
    } else if (n("save")) {
        c.poly({{2, 2}, {11, 2}, {14, 5}, {14, 14}, {2, 14}}, true);
        c.rect(5, 2, 5, 4);
        c.rect(5, 9, 6, 5);
    } else if (n("reopen")) {
        c.poly({{10, 4}, {14, 4}, {14, 14}, {2, 14}, {2, 8}});
        c.poly({{5, 1}, {2, 4}, {5, 7}});
        c.line(2, 4, 9, 4);
    } else {
        return false;
    }
    return true;
}

} // namespace chrome::detail
