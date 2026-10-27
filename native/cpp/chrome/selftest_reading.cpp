// Qt owns mode action/focus/IME lifetime; TCP byte isolation is owned by Rust.
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QDialog>
#include <QInputMethodEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMenuBar>
#include <QPointer>
#include <QStatusBar>
#include <QTimer>
#include <cstdio>

int qt_boot_reading(rust::Str png_out) {
    int argc = 1;
    char arg0[] = "native-reading";
    char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    MainWindow w;
    w.show();
    w.add_tab();
    if (!selftest_ui::activate_window(w)) return 1;
    int verdict = 1;
    QTimer::singleShot(400, &w, [&] {
        bool ok = true;
        auto *view = w.active_view();
        const QPointer<PtyView> retired(view);
        const uint32_t tab = view->tab();
        const rust::String uid = term_uid(tab);
        const int pages = w.page_count();
        auto *local = chrome::find_menu_action(w.menuBar(), QStringLiteral("mode:local"));
        auto *remote = chrome::find_menu_action(w.menuBar(), QStringLiteral("mode:remote"));
        view->inject_ime({}, QStringLiteral("draft"));
        local->trigger();
        QApplication::processEvents();
        ok &= w.active_view() == view && view->tab() == tab && w.page_count() == pages;
        ok &= term_uid(tab) == uid && term_reading(tab) && local->isChecked() && !remote->isChecked();
        ok &= view->preedit().isEmpty() && !view->ime_query(Qt::ImEnabled).toBool();
        view->inject_ime(QStringLiteral("late"), QStringLiteral("late-draft"));
        ok &= view->preedit().isEmpty();
        local->trigger();
        ok &= term_reading(tab); // Selecting the current mode is idempotent.
        std::printf("[reading] menu preserves session, disables IME: %d\n", ok);

        w.split_active(Qt::Horizontal);
        QApplication::processEvents();
        auto *sibling = w.active_view();
        ok &= sibling != view && !term_reading(sibling->tab());
        ok &= !local->isChecked() && remote->isChecked();
        view->setFocus();
        QApplication::processEvents();
        ok &= local->isChecked() && !remote->isChecked();
        QKeyEvent insert(QEvent::KeyPress, Qt::Key_I, Qt::NoModifier, QStringLiteral("i"));
        QApplication::sendEvent(view, &insert);
        w.refresh_chrome();
        ok &= !term_reading(tab) && view->ime_query(Qt::ImEnabled).toBool();
        QKeyEvent toggle(QEvent::KeyPress, Qt::Key_Return, Qt::ControlModifier);
        QApplication::sendEvent(view, &toggle);
        w.refresh_chrome();
        ok &= term_reading(tab) && local->isChecked();
        w.focus_session(w.page_id(0));
        QApplication::processEvents();
        ok &= !local->isChecked() && remote->isChecked();
        w.focus_session(tab);
        QApplication::processEvents();
        ok &= local->isChecked() && !remote->isChecked();
        std::printf("[reading] split/page focus and i/Ctrl+Enter scope: %d\n", ok);
        term_disconnect_tab(tab);
        term_set_reading(tab, false);
        term_clear(tab, 2);
        const QByteArray fixture("\x1b[HREADING fixture\r\nsecond fixture");
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(fixture.data()), fixture.size()));
        term_set_reading(tab, true);
        for (const QChar ch : QStringLiteral("gg0v3l")) {
            QKeyEvent key(QEvent::KeyPress, Qt::Key_unknown, Qt::NoModifier, QString(ch));
            QApplication::sendEvent(view, &key);
        }
        ok &= term_selected_text(tab) == rust::String("READ");
        QTimer::singleShot(0, &w, [&] {
            for (auto *widget : QApplication::topLevelWidgets()) {
                auto *dialog = qobject_cast<QDialog *>(widget);
                if (dialog && dialog->objectName() == QStringLiteral("terminalSearch")) {
                    dialog->findChild<QLineEdit *>(QStringLiteral("searchQuery"))->setText(QStringLiteral("fixture"));
                    dialog->accept();
                }
            }
        });
        QKeyEvent slash(QEvent::KeyPress, Qt::Key_Slash, Qt::NoModifier, QStringLiteral("/"));
        QApplication::sendEvent(view, &slash);
        ok &= term_search_query(tab) == rust::String("fixture") && term_reading(tab);
        std::printf("[reading] keyboard selection and slash search: %d\n", ok);
        if (!png_out.empty()) ok &= w.grab().save(QString::fromUtf8(png_out.data(), int(png_out.size())));

        MainWindow other;
        other.show();
        const uint32_t other_tab = other.active_view()->tab();
        ok &= !term_reading(other_tab);
        while (w.page_count() > 0) w.close_tab(0);
        if (retired) retired->inject_ime(QStringLiteral("retired"), QStringLiteral("retired"));
        ok &= !term_reading(tab) && !term_reading(other_tab);
        other.refresh_chrome();
        std::printf("[reading] retired view and other window scope: %d\n", ok);
        verdict = ok ? 0 : 1;
        app.exit(verdict);
    });
    QTimer::singleShot(15000, &app, [&] { app.exit(2); });
    app.exec();
    return verdict;
}
