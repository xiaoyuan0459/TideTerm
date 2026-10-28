#pragma once
#include <QString>
#include <cstdint>
class MainWindow;
class QAction;
namespace chrome {
QString fold_error(uint8_t code);
void show_fold_regions(MainWindow *window);
bool dispatch_fold(MainWindow *window, QAction *action);
}
