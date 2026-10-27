#pragma once
#include <QApplication>
#include <QClipboard>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QThread>

namespace selftest_ui {
// Publication and an independent Qt/OLE reader do not share an acknowledgement.
// Poll only the exact assertion; never publish again or change the expected text.
inline bool clipboard_matches(const QString &expected, bool trimmed = false) {
    QElapsedTimer deadline;
    deadline.start();
    do {
        const QString actual = QApplication::clipboard()->text();
        if ((trimmed ? actual.trimmed() : actual) == expected) return true;
        QApplication::processEvents(QEventLoop::AllEvents, 5);
        QThread::msleep(1);
    } while (deadline.elapsed() < 300);
    return false;
}
}
