#pragma once
#include <QString>
#include <cstdint>
class MainWindow;
class SendBar;
namespace selftest_ui { bool verify_command_templates(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png); }
