// Optional visual diagnostics of production widgets; no fixtures or configuration writes.
#include "cpp/chrome/ui_capture.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QDir>
#include <QMenu>
#include <QMenuBar>
#include <QTimer>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <cstdio>

namespace chrome {
void capture_ui_if_requested(MainWindow *window) {
    const QString directory = qEnvironmentVariable("TIDETERM_UI_CAPTURE_DIR");
    if (directory.isEmpty()) return;
    QTimer::singleShot(1500, window, [window, directory] {
        QDir output(directory);
        if (!output.isAbsolute() || !output.mkpath(QStringLiteral("."))) return;
        window->grab().save(output.filePath(QStringLiteral("main.png")));
        static const char *names[] = {"shell", "ssh", "telnet", "serial", "tcp"};
        for (uint8_t kind = 0; kind < 5; ++kind) {
            SessionForm form(window, sess_form_get("", kind));
            form.show();
            QApplication::processEvents();
            form.grab().save(output.filePath(QString::fromLatin1(names[kind]) + QStringLiteral(".png")));
            if (kind == 1) {
                if (auto *tree = form.findChild<QTreeWidget *>()) {
                    int page = 0;
                    for (QTreeWidgetItemIterator item(tree); *item; ++item) {
                        if ((*item)->isDisabled() || (*item)->childCount() > 0) continue;
                        tree->setCurrentItem(*item);
                        QApplication::processEvents();
                        form.grab().save(output.filePath(QStringLiteral("ssh-page-%1.png").arg(++page)));
                    }
                }
            }
        }
        SessionForm defaults(window, sess_defaults_get(1), true);
        defaults.show();
        QApplication::processEvents();
        defaults.grab().save(output.filePath(QStringLiteral("defaults.png")));
        std::printf("[ui] captured production widgets in %s\n", qPrintable(output.absolutePath()));
    });
}
}
