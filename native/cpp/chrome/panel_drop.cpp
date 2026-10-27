#include "cpp/chrome/panel_drop.h"
#include "cpp/chrome/panel_controls.h"
#include <QDockWidget>
#include <QGuiApplication>
#include <QMainWindow>
#include <QMenuBar>
#include <QScreen>
#include <QStatusBar>
#include <algorithm>

namespace chrome {
namespace {
QRect bounds(QWidget *widget, QMainWindow *window) {
    return QRect(widget->mapTo(window, QPoint()), widget->size());
}
PanelDrop::Kind edge(const QRect &rect, const QPoint &point, bool center_tab) {
    const qreal x = qreal(point.x() - rect.left()) / qMax(1, rect.width());
    const qreal y = qreal(point.y() - rect.top()) / qMax(1, rect.height());
    const qreal nearest = std::min({x, 1 - x, y, 1 - y});
    if (center_tab && nearest >= 0.28) return PanelDrop::Tab;
    if (!center_tab && x >= 0.25 && x <= 0.75)
        return y < 0.5 ? PanelDrop::Top : PanelDrop::Bottom;
    if (nearest == x) return PanelDrop::Left;
    if (nearest == 1 - x) return PanelDrop::Right;
    return nearest == y ? PanelDrop::Top : PanelDrop::Bottom;
}
QRect half(QRect rect, PanelDrop::Kind kind) {
    if (kind == PanelDrop::Left) rect.setWidth(rect.width() / 2);
    if (kind == PanelDrop::Right) rect.setLeft(rect.left() + rect.width() / 2);
    if (kind == PanelDrop::Top) rect.setHeight(rect.height() / 2);
    if (kind == PanelDrop::Bottom) rect.setTop(rect.top() + rect.height() / 2);
    return rect;
}
Qt::DockWidgetArea dock_area(PanelDrop::Kind kind) {
    if (kind == PanelDrop::Left) return Qt::LeftDockWidgetArea;
    if (kind == PanelDrop::Right) return Qt::RightDockWidgetArea;
    if (kind == PanelDrop::Top) return Qt::TopDockWidgetArea;
    return Qt::BottomDockWidgetArea;
}
}

PanelDrop panel_drop_target(QMainWindow *window, QDockWidget *source, const QPoint &global) {
    PanelDrop drop;
    if (!window || !source) return drop;
    const QPoint point = window->mapFromGlobal(global);
    if (!window->rect().contains(point)) {
        if (source->features().testFlag(QDockWidget::DockWidgetFloatable)) drop.kind = PanelDrop::Float;
        return drop;
    }
    // Only exposed dock content is a target (inactive tabs can still be isVisible()).
    for (auto *target : window->findChildren<QDockWidget *>(QString(), Qt::FindDirectChildrenOnly)) {
        if (target == source || target->isFloating() || !target->isVisible() || target->visibleRegion().isEmpty()) continue;
        const QRect rect = bounds(target, window);
        if (!rect.contains(point)) continue;
        const auto area = window->dockWidgetArea(target);
        if (!source->isAreaAllowed(area)) return drop;
        drop.kind = edge(rect, point, true);
        drop.target = target;
        drop.preview = half(rect, drop.kind);
        return drop;
    }
    if (auto *central = window->centralWidget(); central && bounds(central, window).contains(point)) {
        const QRect rect = bounds(central, window);
        const auto side = edge(rect, point, false);
        drop.kind = PanelDrop::Area;
        drop.area = dock_area(side);
        drop.preview = half(rect, side);
        // Respect QMainWindow's corner ownership, including the full-width send area.
        if (side == PanelDrop::Bottom) {
            if (window->corner(Qt::BottomLeftCorner) == drop.area) drop.preview.setLeft(0);
            if (window->corner(Qt::BottomRightCorner) == drop.area) drop.preview.setRight(window->width() - 1);
        }
        if (side == PanelDrop::Top) {
            if (window->corner(Qt::TopLeftCorner) == drop.area) drop.preview.setLeft(0);
            if (window->corner(Qt::TopRightCorner) == drop.area) drop.preview.setRight(window->width() - 1);
        }
        if (!source->isAreaAllowed(drop.area)) return {};
        return drop;
    }
    // Returning over the original dock keeps it in place; chrome is not a drop zone.
    return drop;
}

void apply_panel_drop(QMainWindow *window, QDockWidget *source, const PanelDrop &drop,
                      const QPoint &global) {
    if (!window || !source || drop.kind == PanelDrop::None) return;
    if (drop.kind == PanelDrop::Float) {
        const QSize size = source->isFloating() ? source->size() :
            QSize(qBound(240, source->width(), 520), qBound(160, source->height(), 420));
        source->setFloating(true);
        source->resize(size);
        QPoint position = global - QPoint(50, 12);
        if (auto *screen = QGuiApplication::screenAt(global)) {
            const QRect available = screen->availableGeometry();
            position.setX(qBound(available.left(), position.x(), qMax(available.left(), available.right() - size.width())));
            position.setY(qBound(available.top(), position.y(), qMax(available.top(), available.bottom() - size.height())));
        }
        source->move(position);
    } else if (drop.kind == PanelDrop::Area) {
        if (!source->isAreaAllowed(drop.area)) return;
        window->removeDockWidget(source);
        // Register the destination first: an unregistered floating dock has no
        // layout slot to return to when setFloating(false) runs.
        window->addDockWidget(drop.area, source);
        source->setFloating(false);
        const bool vertical = drop.area == Qt::TopDockWidgetArea || drop.area == Qt::BottomDockWidgetArea;
        window->resizeDocks({source}, {vertical ? drop.preview.height() : drop.preview.width()},
            vertical ? Qt::Vertical : Qt::Horizontal);
    } else if (auto *target = drop.target.data()) {
        if (!source->isAreaAllowed(window->dockWidgetArea(target))) return;
        if (drop.kind == PanelDrop::Tab) {
            window->removeDockWidget(source);
            window->tabifyDockWidget(target, source);
            source->setFloating(false);
        } else {
            const bool vertical = drop.kind == PanelDrop::Top || drop.kind == PanelDrop::Bottom;
            split_panel(window, target, source, vertical ? Qt::Vertical : Qt::Horizontal);
            if (drop.kind == PanelDrop::Left || drop.kind == PanelDrop::Top)
                split_panel(window, source, target, vertical ? Qt::Vertical : Qt::Horizontal);
        }
    } else return;
    source->show();
    source->raise();
    if (source->widget()) source->widget()->setFocus();
}
}
