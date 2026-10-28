// Independently drawn WindTerm-style UI icons; one geometry for every DPI.
#include "cpp/chrome/icon_shapes.h"
#include "cpp/chrome/icon_canvas.h"

namespace chrome::detail {

void paint_icon_shape(QPainter &p, const QString &name, qreal size, const QColor &ink, qreal dpr) {
    p.save();
    IconCanvas c(p, size, ink, dpr);
    const auto n = [&](const char *value) { return name == QLatin1String(value); };
    if (n("more")) {
        for (int y : {3, 8, 13}) c.dot(8, y);
    } else if (n("menu")) {
        for (int y : {4, 8, 12}) c.line(3, y, 13, y);
    } else if (n("hdr_menu")) {
        for (int y : {3, 7, 11}) { c.dot(3, y); c.dot(7, y); }
        c.poly({{10, 10}, {14, 10}, {12, 13}}, true, true);
    } else if (n("add")) {
        c.plus(8, 8, 5);
    } else if (n("remove") || n("minimize")) {
        c.line(3, 8, 13, 8);
    } else if (n("close")) {
        c.cross(8, 8, 4);
    } else if (n("fold")) {
        c.poly({{4, 10}, {8, 6}, {12, 10}});
    } else if (n("down")) {
        c.poly({{4, 6}, {8, 10}, {12, 6}});
    } else if (n("chevron_right")) {
        c.poly({{6, 3}, {11, 8}, {6, 13}});
    } else if (n("up")) {
        c.line(8, 14, 8, 2);
        c.poly({{4, 6}, {8, 2}, {12, 6}});
    } else if (n("play")) {
        c.poly({{5, 3}, {13, 8}, {5, 13}}, true, true);
    } else if (n("stop")) {
        c.rect(4, 4, 8, 8, true);
    } else if (n("shell_powershell")) {
        IconCanvas tile(p, size, QColor(0x24, 0x78, 0xb8), dpr);
        tile.poly({{4, 2}, {15, 2}, {12, 14}, {1, 14}}, true, true);
        IconCanvas glyph(p, size, QColor(Qt::white), dpr);
        glyph.poly({{5, 5}, {8, 8}, {4, 11}});
        glyph.line(8, 11, 11, 11);
    } else if (n("shell_cmd")) {
        c.rect(1, 3, 14, 10);
        c.line(1, 5, 15, 5);
        c.poly({{4, 7}, {6, 9}, {4, 11}}); c.line(8, 11, 11, 11);
    } else if (n("shell_bash")) {
        c.poly({{8, 1}, {14, 4}, {14, 12}, {8, 15}, {2, 12}, {2, 4}}, true);
        c.poly({{5, 5}, {8, 8}, {5, 11}}); c.line(9, 11, 11, 11);
    } else if (n("shell_zsh")) {
        c.rect(1, 3, 14, 10);
        c.poly({{3, 6}, {6, 8}, {3, 10}}); c.poly({{8, 6}, {11, 8}, {8, 10}});
    } else if (n("shell_fish")) {
        c.ellipse(2, 4, 10, 8); c.poly({{12, 8}, {15, 4}, {15, 12}}, true); c.dot(5, 7);
    } else if (n("terminal")) {
        c.terminal();
    } else if (n("remote_host")) {
        c.remote_host();
    } else if (n("pane_add") || n("pane_close") || n("pane_split") || n("pane_reconnect")) {
        c.poly({{9, 4}, {2, 4}, {2, 14}, {12, 14}, {12, 8}});
        if (n("pane_add")) c.plus(12, 3, 2);
        else if (n("pane_close")) c.cross(12, 3, 2);
        else if (n("pane_reconnect")) c.ellipse(10, 1, 4, 4);
        else { c.line(7, 5, 7, 13); c.ellipse(10, 1, 4, 4); }
    } else if (n("split_h") || n("split_v") || n("split")) {
        c.rect(2, 3, 12, 10);
        // Qt::Horizontal creates left/right panes; match the real menu action.
        if (n("split_h") || n("split")) c.line(8, 3, 8, 13);
        else c.line(2, 8, 14, 8);
    } else if (n("zoom") || n("fullscreen")) {
        c.poly({{2, 6}, {2, 2}, {6, 2}});
        c.poly({{10, 2}, {14, 2}, {14, 6}});
        c.poly({{14, 10}, {14, 14}, {10, 14}});
        c.poly({{6, 14}, {2, 14}, {2, 10}});
        if (n("zoom")) { c.line(3, 3, 6, 6); c.line(10, 10, 13, 13); }
    } else if (n("maximize")) {
        c.rect(2, 2, 12, 12);
    } else if (n("restore") || n("float")) {
        c.poly({{5, 3}, {5, 1}, {14, 1}, {14, 10}, {12, 10}});
        c.rect(2, 5, 8, 9);
        if (n("float")) c.line(2, 8, 10, 8);
    } else if (n("dock")) {
        c.rect(2, 2, 12, 12);
        c.line(6, 2, 6, 14);
        c.poly({{9, 6}, {7, 8}, {9, 10}});
        c.line(7, 8, 12, 8);
    } else if (n("info")) {
        c.ellipse(2, 2, 12, 12);
        c.dot(8, 5);
        c.line(8, 8, 8, 11);
        c.line(7, 11, 9, 11);
    } else if (n("settings")) {
        // Six solid teeth and a clear centre hole remain legible at panel size.
        QPainterPath gear;
        for (int i = 0; i < 24; ++i) {
            const int corner = i % 4;
            const qreal angle = ((i / 4) * 60 + (corner == 0 ? -25 : corner == 1 ? -15 :
                corner == 2 ? 15 : 25) - 90) * 0.01745329252;
            const qreal radius = corner == 0 || corner == 3 ? 4.6 : 6.6;
            const QPointF point = c.at(8 + radius * std::cos(angle), 8 + radius * std::sin(angle));
            if (i == 0) gear.moveTo(point); else gear.lineTo(point);
        }
        gear.closeSubpath();
        gear.addEllipse(c.box(5.5, 5.5, 5, 5));
        gear.setFillRule(Qt::OddEvenFill);
        p.setPen(Qt::NoPen);
        p.setBrush(ink);
        p.drawPath(gear);
    } else if (n("star")) {
        c.poly({{8, 1}, {10, 5}, {14, 6}, {11, 9}, {12, 14},
                {8, 12}, {4, 14}, {5, 9}, {2, 6}, {6, 5}}, true);
    } else if (n("view") || n("view_off")) {
        QPainterPath eye;
        eye.moveTo(c.at(1, 8));
        eye.quadTo(c.at(8, 0), c.at(15, 8));
        eye.quadTo(c.at(8, 16), c.at(1, 8));
        p.drawPath(eye);
        c.ellipse(6, 6, 4, 4);
        if (n("view_off")) c.line(2, 14, 14, 2);
    } else if (n("lock")) {
        c.round_rect(3, 7, 10, 7);
        c.poly({{5, 7}, {5, 4}, {6, 2}, {10, 2}, {11, 4}, {11, 7}});
        c.line(8, 10, 8, 12);
    } else if (n("sessions")) {
        c.rect(2, 2, 12, 12);
        c.line(2, 5, 14, 5);
        c.line(6, 5, 6, 14);
        c.line(8, 8, 12, 8);
        c.line(8, 11, 12, 11);
    } else if (n("history")) {
        c.ellipse(3, 2, 11, 12);
        c.poly({{8, 5}, {8, 8}, {11, 10}});
        c.poly({{1, 3}, {1, 7}, {5, 7}});
    } else if (n("transfer")) {
        c.line(2, 5, 14, 5);
        c.poly({{11, 2}, {14, 5}, {11, 8}});
        c.line(14, 11, 2, 11);
        c.poly({{5, 8}, {2, 11}, {5, 14}});
    } else if (n("send")) {
        c.poly({{2, 2}, {14, 8}, {2, 14}, {5, 8}}, true);
        c.line(5, 8, 14, 8);
    } else if (!paint_action_icon(c, name)) {
        paint_file_icon(c, name);
    }
    p.restore();
}

} // namespace chrome::detail
