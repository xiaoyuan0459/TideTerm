#pragma once
#include <QString>
class MainWindow;
namespace chrome { void show_command_palette(MainWindow *window, const QString &initial = QStringLiteral(">")); }
