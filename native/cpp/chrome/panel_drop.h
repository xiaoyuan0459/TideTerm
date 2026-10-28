#pragma once
#include <QPointer>
#include <QRect>
#include <Qt>
class QDockWidget;
class QMainWindow;

namespace chrome {
// Preview and release use this same destination; resolving never changes layout.
struct PanelDrop {
    enum Kind { None, Float, Tab, Left, Right, Top, Bottom, Area } kind = None;
    QPointer<QDockWidget> target;
    Qt::DockWidgetArea area = Qt::NoDockWidgetArea;
    QRect preview; // Main-window coordinates.
};
PanelDrop panel_drop_target(QMainWindow *window, QDockWidget *source, const QPoint &global);
void apply_panel_drop(QMainWindow *window, QDockWidget *source, const PanelDrop &drop,
                      const QPoint &global);
}
