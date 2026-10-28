#pragma once
#include <QString>
class MainWindow;
namespace selftest_ui {
bool verify_window_close(MainWindow &window, const QString &png);
bool accept_window_close(MainWindow &window);
bool verify_close_preference(MainWindow &survivor);
bool capture_main_menus(MainWindow &window, const QString &png);
}
