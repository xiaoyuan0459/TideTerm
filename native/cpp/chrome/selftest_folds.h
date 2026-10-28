#pragma once
#include <QString>
class MainWindow;
class PtyView;
namespace selftest_ui { bool verify_folds(MainWindow &window, PtyView *view, const QString &png); }
