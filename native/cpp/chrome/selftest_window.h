#pragma once
#include <QApplication>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QThread>
#include <QWidget>

namespace selftest_ui {
// Synthetic focus/drag input needs a mapped, active window. A show request
// alone is asynchronous on desktop backends and does not establish focus.
inline bool activate_window(QWidget &window) {
    window.raise();
    window.activateWindow();
    QElapsedTimer deadline;
    deadline.start();
    do {
        QApplication::processEvents(QEventLoop::AllEvents, 10);
        auto *focus = QApplication::focusWidget();
        if (window.isActiveWindow() && focus &&
            (focus == &window || window.isAncestorOf(focus))) return true;
        QThread::msleep(10);
    } while (deadline.elapsed() < 2000);
    return false;
}
}
