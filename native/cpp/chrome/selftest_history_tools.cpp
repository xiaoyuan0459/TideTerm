// Normal menu entry, bounded visible pages, cancel, failure and source retirement.
#include "cpp/chrome/term_menu.h"
#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QDialog>
#include <QFileDialog>
#include <QLabel>
#include <QMenu>
#include <QPointer>
#include <QPushButton>
#include <QTimer>
#include <QThread>
#include <cstdio>
#include <chrono>
#include "cpp/i18n.h"

int qt_boot_history(rust::Str png_out) {
    QApplication::setAttribute(Qt::AA_DontUseNativeDialogs);
    int argc = 1; char arg0[] = "native-history"; char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings;
    i18n::install_from_env();
    MainWindow window; window.show(); window.add_tab();
    if (!selftest_ui::activate_window(window)) return 1;
    int verdict = 2;
    QTimer::singleShot(400, &window, [&] {
        auto *view = window.active_view();
        const uint32_t tab = view->tab();
        term_disconnect_tab(tab); term_clear(tab, 2);
        const QByteArray input("SYNTHETIC_HISTORY_PAGE\r\nsecond page fixture");
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(input.data()), size_t(input.size())));
        QMenu *menu = chrome::build_term_menu(view);
        auto *entry = menu->findChild<QAction *>(QStringLiteral("historyToolsAction"));
        bool ok = entry && entry->isEnabled();
        if (entry) entry->trigger();
        QPointer<QDialog> dialog(view->findChild<QDialog *>(QStringLiteral("historyTools")));
        ok &= !dialog.isNull();
        if (dialog) {
            auto *browse = dialog->findChild<QPushButton *>(QStringLiteral("historyBrowse"));
            auto *text = dialog->findChild<QLabel *>(QStringLiteral("historyPageText"));
            browse->click();
            const auto end = std::chrono::steady_clock::now() + std::chrono::seconds(5);
            while (text->text().isEmpty() && std::chrono::steady_clock::now() < end) { QApplication::processEvents(); QThread::msleep(5); }
            ok &= text->text().contains(QStringLiteral("SYNTHETIC_HISTORY_PAGE")) && text->text().size() <= 65540;
            QTimer::singleShot(0, &window, [] {
                for (auto *widget : QApplication::topLevelWidgets())
                    if (auto *file = dynamic_cast<QFileDialog *>(widget)) file->reject();
            });
            dialog->findChild<QPushButton *>(QStringLiteral("historySave"))->click();
            ok &= history_export_poll(tab).state == 2;
            const QByteArray alternate("\x1b[?1049h");
            term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(alternate.data()), size_t(alternate.size())));
            browse->click();
            ok &= !dialog->findChild<QLabel *>(QStringLiteral("historyStatus"))->text().isEmpty();
            if (!png_out.empty()) ok &= dialog->grab().save(QString::fromUtf8(png_out.data(), int(png_out.size())));
            term_set_locked(true);
            const auto close_end = std::chrono::steady_clock::now() + std::chrono::seconds(2);
            while (dialog && std::chrono::steady_clock::now() < close_end) { QApplication::processEvents(); QThread::msleep(5); }
            term_set_locked(false);
            ok &= dialog.isNull() && history_export_poll(tab).state == 0;
        }
        auto *tmux = menu->findChild<QAction *>(QStringLiteral("tmuxToolsAction"));
        if (tmux) tmux->trigger(); else ok = false;
        auto *tmux_dialog = view->findChild<QDialog *>(QStringLiteral("tmuxTools"));
        ok &= tmux_dialog != nullptr;
        if (tmux_dialog) {
            tmux_dialog->findChild<QPushButton *>(QStringLiteral("tmuxPreview"))->click();
            ok &= !tmux_dialog->findChild<QPushButton *>(QStringLiteral("tmuxSend"))->isEnabled();
            tmux_dialog->close();
        }
        delete menu;
        std::printf("[history] normal menu, temporary page, save cancel, failure and lock retirement: %d\n", ok);
        verdict = ok ? 0 : 1; app.exit(verdict);
    });
    QTimer::singleShot(15000, &app, [&] { app.exit(2); });
    app.exec(); return verdict;
}
