#pragma once
class MainWindow;
class PtyView;
class QAction;
namespace chrome {
bool dispatch_selection(MainWindow *window, QAction *action);
void refresh_selection_actions(MainWindow *window, PtyView *view);
}
