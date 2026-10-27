#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include <QApplication>
#include <QTemporaryDir>
#include <QTabWidget>
#include <QPixmap>
#include <QTimer>
#include <QMessageBox>
#include <QAbstractButton>
#include <cstdio>

int qt_boot_workspace(rust::Str png_out) {
    int argc = 1;
    char name[] = "native-workspace";
    char *argv[] = {name, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings;
    QTemporaryDir directory;
    if (!directory.isValid()) return 1;
    const auto path = directory.path().toUtf8();
    sess_test_store_dir(rust::Str(path.constData(), size_t(path.size())));
    i18n::install_from_env();
    bool ok = true;
    {
        MainWindow original(false);
        auto cfg = sess_form_get(rust::Str(""), 0);
        cfg.local_cwd = rust::String(directory.path().toUtf8().toStdString());
#ifdef Q_OS_WIN
        cfg.local_shell = rust::String("cmd");
        cfg.local_args = rust::String("/Q /K");
#else
        cfg.local_shell = rust::String("bash");
        cfg.local_args = rust::String("--noprofile --norc");
#endif
        cfg.name = rust::String("first");
        original.add_tab_with(sess_open_draft(cfg));
        cfg.name = rust::String("second");
        original.add_tab_with(sess_open_draft(cfg));
        original.show(); QApplication::processEvents();
        original.split_active(Qt::Horizontal);
        original.split_group(Qt::Vertical);
        original.close_tab(1);
        ok &= original.page_count() == 1;
        original.reopen_closed_tab();
        ok &= original.page_count() == 2 && original.pane_ids(1).size() == 2;
        original.split_group(Qt::Horizontal);
        QApplication::processEvents();
        const auto expected_pages = original.page_count();
        const auto expected_groups = original.workspace()->groups().size();
        ok &= original.save_workspace();
        const auto keys = workspace_list();
        ok &= keys.size() == 1;
        if (!keys.empty()) {
            const auto snapshot = workspace_load(keys[0]);
            MainWindow restored(false);
            restored.show();
            ok &= restored.restore_workspace(QByteArray(snapshot.data(), int(snapshot.size())));
            QApplication::processEvents();
            ok &= restored.page_count() == expected_pages && restored.workspace()->groups().size() == expected_groups;
            size_t panes = 0;
            for (int page = 0; page < restored.page_count(); ++page) {
                for (auto id : restored.pane_ids(page)) {
                    ++panes;
                    const auto active = sess_active_config(id);
                    ok &= active.local_cwd == cfg.local_cwd;
                }
            }
            ok &= panes == 3 && original.page_count() == expected_pages;
            MainWindow declined(false);
            QTimer::singleShot(0, &declined, [] {
                if (auto *box = dynamic_cast<QMessageBox *>(QApplication::activeModalWidget())) box->button(QMessageBox::No)->click();
            });
            ok &= !declined.restore_previous_workspace() && declined.page_count() == 0;
            MainWindow startup(false);
            QTimer::singleShot(0, &startup, [] {
                if (auto *box = dynamic_cast<QMessageBox *>(QApplication::activeModalWidget())) box->button(QMessageBox::Yes)->click();
            });
            ok &= startup.restore_previous_workspace() && startup.page_count() == expected_pages;
            MainWindow rejected(false);
            ok &= !rejected.restore_workspace(QByteArray("{\"version\":99}")) && rejected.page_count() == 0;
            const QString png = QString::fromUtf8(png_out.data(), int(png_out.size()));
            if (!png.isEmpty()) restored.grab().save(png);
        }
    }
    QApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    sess_test_store_reset();
    std::printf("[workspace] restore topology, closed-tab lifecycle, independent window and invalid file: %s\n", ok ? "PASS" : "FAIL");
    return ok ? 0 : 1;
}
