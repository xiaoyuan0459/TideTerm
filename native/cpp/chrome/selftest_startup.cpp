// Real isolated shell output, source-fold clicks and window chrome; engine coordinates have their own owner.
#include "cpp/chrome/selftest_startup.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QAction>
#include <QDialog>
#include <QPushButton>
#include <QTimer>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QLineEdit>
#include <QMouseEvent>
#include <QTabWidget>
#include <QTemporaryDir>
#include <QThread>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
void click(PtyView *view, const FoldMarker &marker) {
    const QPointF origin = -view->grid_pt(QPointF());
    const QPointF local = origin + QPointF(marker.x + marker.w / 2, marker.y + marker.h / 2);
    const QPointF global = view->mapToGlobal(local.toPoint());
    QMouseEvent press(QEvent::MouseButtonPress, local, global, Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QApplication::sendEvent(view, &press);
    QMouseEvent release(QEvent::MouseButtonRelease, local, global, Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(view, &release);
    QApplication::processEvents();
}
}

bool selftest_ui::verify_startup(const QString &png) {
    const QString root = QDir(QCoreApplication::applicationDirPath()).absoluteFilePath(QStringLiteral("../fixtures"));
    QDir().mkpath(root);
    QTemporaryDir fixture(root + QStringLiteral("/startup-XXXXXX"));
    if (!fixture.isValid()) return false;
    MainWindow window(false);
    auto cfg = sess_form_get(rust::Str(""), 0);
    cfg.local_cwd = rs(fixture.path());
#ifdef Q_OS_WIN
    cfg.local_shell = rust::String("pwsh"); cfg.name = rust::String("PowerShell");
#else
    QFile rc(fixture.filePath(QStringLiteral("init.sh")));
    if (!rc.open(QIODevice::WriteOnly)) return false;
    rc.write("printf 'Shell startup fixture\\nCopyright fixture\\n\\n'\nPS1='\\[\\e]133;A\\a\\]fixture$ \\[\\e]133;B\\a\\]'\n"); rc.close();
    cfg.local_shell = rust::String("bash"); cfg.name = rust::String("Bash");
    cfg.local_args = rs(QStringLiteral("--noprofile --rcfile '") + rc.fileName() + QStringLiteral("' -i"));
#endif
    const uint32_t tab = sess_open_draft(cfg);
    if (tab == UINT32_MAX) return false;
    window.add_tab_with(tab); window.resize(1400, 760); window.show();
    if (!activate_window(window)) return false;
    QElapsedTimer wait; wait.start(); uint64_t startup = 0; bool prompt = false;
    while (wait.elapsed() < 12000 && (!startup || !prompt)) {
        term_pump(); QApplication::processEvents();
        for (const auto &row : term_commands(tab)) if (row.startup && row.foldable) startup = row.id;
#ifdef Q_OS_WIN
        const QString screen = q(term_screen_text(tab)).trimmed();
        prompt = screen.contains(QStringLiteral("PS ")) && screen.endsWith(QLatin1Char('>'));
#else
        prompt = q(term_screen_text(tab)).contains(QStringLiteral("fixture$ "));
#endif
        if (!startup || !prompt) QThread::msleep(10);
    }
    if (!startup || !prompt) {
        std::printf("[commands] real startup source block: 0 rows=%zu\n", term_commands(tab).size());
        if (!png.isEmpty()) window.workspace()->first_group()->grab().save(png + QStringLiteral(".startup-missing.png"));
        return false;
    }
    auto *view = window.active_view(); const auto uid = term_uid(tab);
    const auto output = term_command_output(tab, startup, false);
    const auto history = history_revision(tab);
    auto *draft = window.page_sendbar(window.current_page())->input(); draft->setText(QStringLiteral("STARTUP_DRAFT"));
    bool folded = false, reopened = false;
    for (const auto &marker : term_fold_markers(tab)) if (marker.id == startup) { click(view, marker); break; }
    for (const auto &row : term_commands(tab)) if (row.id == startup) folded = row.folded;
    if (!png.isEmpty()) window.workspace()->first_group()->grab().save(png + QStringLiteral(".startup-folded.png"));
    for (const auto &marker : term_fold_markers(tab)) if (marker.id == startup) { click(view, marker); break; }
    for (const auto &row : term_commands(tab)) if (row.id == startup) reopened = !row.folded;
    const bool intact = term_uid(tab) == uid && history_revision(tab) == history &&
        q(term_command_output(tab, startup, false).text) == q(output.text) && draft->text() == QStringLiteral("STARTUP_DRAFT");
    bool no_command = false;
    if (auto *action = chrome::find_menu_action(window.menuBar(), QStringLiteral("goto:command-outline"))) {
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("commandOutline")); if (!dialog) return;
            auto *copy = dialog->findChild<QPushButton *>(QStringLiteral("outlineCopyCommand"));
            auto *insert = dialog->findChild<QPushButton *>(QStringLiteral("outlineInsert"));
            no_command = copy && insert && !copy->isEnabled() && !insert->isEnabled();
            if (insert) insert->click();
            no_command &= draft->text() == QStringLiteral("STARTUP_DRAFT"); dialog->reject();
        });
        action->trigger();
    }
    auto second = sess_form_get(rust::Str(""), 0); second.local_cwd = rs(fixture.path());
#ifdef Q_OS_WIN
    second.local_shell = rust::String("cmd"); second.name = rust::String("Command Prompt");
#else
    second.local_path = rust::String("/bin/sh"); second.name = rust::String("POSIX sh");
#endif
    const auto second_tab = sess_open_draft(second);
    if (second_tab != UINT32_MAX) window.add_tab_with(second_tab);
    auto remote = sess_form_get(rust::Str(""), 1); remote.name = rust::String("SSH fixture");
    remote.ssh_host = rust::String("127.0.0.1"); remote.ssh_port = 1; remote.ssh_user = rust::String("fixture");
    const auto ssh = sess_open_draft(remote);
    if (ssh != UINT32_MAX) window.add_tab_with(ssh);
    window.focus_session(tab); window.refresh_chrome(); QApplication::processEvents();
    if (!png.isEmpty()) window.workspace()->first_group()->grab().save(png + QStringLiteral(".startup-expanded.png"));
    std::printf("[commands] real startup source/gutter fold/reopen/output/draft/non-command: %d/%d/%d/%d/%d\n", startup != 0, folded, reopened, intact, no_command);
    return folded && reopened && intact && no_command && second_tab != UINT32_MAX && ssh != UINT32_MAX;
}
