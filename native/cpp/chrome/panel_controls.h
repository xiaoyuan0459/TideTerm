#pragma once
#include <QList>
#include <QPoint>
#include <Qt>
class QDockWidget;
class QMainWindow;
class QMenu;
namespace chrome {
QList<QDockWidget *> managed_panels(QMainWindow *window);
void fill_panel_menu(QMenu *menu, QDockWidget *source);
void split_panel(QMainWindow *window, QDockWidget *source, QDockWidget *target, Qt::Orientation orientation);
void install_panel_shortcuts(QMainWindow *window);
}
