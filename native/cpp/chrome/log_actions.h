#pragma once
class QAction;
class MainWindow;
namespace chrome {
bool dispatch_log(MainWindow *window, QAction *action);
void show_log_settings(MainWindow *window);
void refresh_log_actions(MainWindow *window);
}
