#pragma once
#include <QPointF>
#include <functional>
class MainWindow;
class PtyView;
namespace selftest_ui {
bool verify_selection_tools(MainWindow &window, PtyView *view, const std::function<QPointF(int,int)> &at);
}
