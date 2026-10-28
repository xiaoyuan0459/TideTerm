#include "cpp/bridge_shim.h"
#include "cpp/clipboard.h"
#include <QApplication>
#include <QClipboard>
#include <QMimeData>
#include <QThread>
#include <QWidget>
#ifdef Q_OS_WIN
#include "cpp/clipboard_windows.h"
#endif

namespace {
QClipboard *clipboard() {
    auto *app = qobject_cast<QApplication *>(QCoreApplication::instance());
    return app && QThread::currentThread() == app->thread() ? QApplication::clipboard() : nullptr;
}
}
bool host_clipboard::copy_text(const QString &value) {
    auto *clip = clipboard();
    if (!clip) return false;
#ifdef Q_OS_WIN
    if (QGuiApplication::platformName() == QLatin1String("windows")) {
        QWidget *owner = QApplication::activeWindow();
        if (!owner) for (auto *widget : QApplication::topLevelWidgets()) {
            if (widget->isVisible()) { owner = widget; break; }
        }
        return owner && publish_unicode(reinterpret_cast<HWND>(owner->winId()), value);
    }
#endif
    clip->setText(value);
    return clip->text() == value;
}
bool qt_clipboard_copy(rust::Str text) {
    return host_clipboard::copy_text(QString::fromUtf8(text.data(), int(text.size())));
}
bool qt_has_gui_application() {
    return qobject_cast<QApplication *>(QCoreApplication::instance()) != nullptr;
}
QString host_clipboard::read_text() {
    auto *clip = clipboard();
    if (!clip) return {};
#ifdef Q_OS_WIN
    if (QGuiApplication::platformName() == QLatin1String("windows")) return read_unicode();
#endif
    return clip->text();
}
rust::String qt_clipboard_read() {
    return rust::String(host_clipboard::read_text().toUtf8().toStdString());
}
bool qt_clipboard_has_image() {
    auto *clip = clipboard();
    return clip && clip->mimeData() && clip->mimeData()->hasImage();
}
