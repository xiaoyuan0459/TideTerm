#pragma once
#include <QString>
class QDialog;
class MainWindow;
class PtyView;
namespace selftest_ui {
bool verify_outline_hover(QDialog *dialog, PtyView *view, const QString &png);
bool verify_history_scroll(MainWindow &window, PtyView *view, const QString &png);
bool verify_history_geometry(PtyView *view);
}
