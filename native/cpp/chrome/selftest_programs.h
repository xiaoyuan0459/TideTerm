#pragma once
#include <QString>
#include <cstdint>
class MainWindow;
class SendBar;
namespace selftest_ui {
bool verify_programs(MainWindow &window, SendBar *bar, uint32_t tab, const QString &directory, const QString &png);
bool verify_ssh_programs(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png);
}
