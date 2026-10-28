// Cross-language risks: active-pane actions, dialog lifetime and real shell Enter.
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_clipboard.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/selftest_history.h"
#include "cpp/chrome/selftest_folds.h"
#include "cpp/chrome/selftest_marks.h"
#include "cpp/chrome/selftest_palette.h"
#include "cpp/chrome/selftest_startup.h"
#include "cpp/chrome/selftest_multiline.h"
#include "cpp/chrome/selftest_senders.h"
#include "cpp/chrome/selftest_completion.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QDialog>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenuBar>
#include <QPushButton>
#include <QTimer>
#include <cstdio>
#ifdef Q_OS_WIN
#include <qt_windows.h>
#endif

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
}

int qt_boot_commands(rust::Str png_out) {
    (void)std::setvbuf(stdout, nullptr, _IONBF, 0);
    int argc = 1;
    char arg0[] = "native-commands";
    char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    MainWindow window;
    window.show();
    if (!selftest_ui::activate_window(window)) return 1;
    bool ok = true;
    int verdict = 1;
    QTimer::singleShot(500, &window, [&] {
        auto *view = window.active_view();
        const uint32_t tab = view->tab();
        const rust::String uid = term_uid(tab);
        term_disconnect_tab(tab); term_clear(tab, 2);
        const QByteArray bytes("\x1b]133;A\007$ \x1b]133;B\007echo FIRST\r\n\x1b]133;C\007FIRST output\r\nHIDDEN window line\r\nLAST first output\r\n\x1b]133;D;0\007"
            "\x1b]133;A\007$ \x1b]133;B\007echo SECOND\r\n\x1b]133;C\007SECOND output\r\n\x1b]133;D;0\007\x1b]133;A\007$ ");
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
        auto *outline = chrome::find_menu_action(window.menuBar(), QStringLiteral("goto:command-outline"));
        auto *previous = chrome::find_menu_action(window.menuBar(), QStringLiteral("goto:previous-command"));
        if (!outline || !previous) { app.exit(1); return; }
        ok &= outline->isEnabled() && previous->isEnabled();
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("commandOutline"));
            if (!dialog) { ok = false; return; }
            auto *list = dialog->findChild<QListWidget *>(QStringLiteral("outlineList"));
            auto *filter = dialog->findChild<QLineEdit *>(QStringLiteral("outlineFilter"));
            ok &= list && list->count() == 2;
            filter->setText(QStringLiteral("FIRST"));
            ok &= list->currentItem() && list->currentItem()->text().contains(QStringLiteral("FIRST"));
            ok &= dialog->findChild<QLabel *>(QStringLiteral("outlinePreview"))->text() == QStringLiteral("FIRST output\nHIDDEN window line\nLAST first output");
            ok &= selftest_ui::verify_outline_hover(dialog, view,
                QString::fromUtf8(png_out.data(), int(png_out.size())));
            dialog->findChild<QPushButton *>(QStringLiteral("outlineCopyCommand"))->click();
            const bool command_copy = selftest_ui::clipboard_matches(QStringLiteral("echo FIRST"));
            ok &= command_copy;
            // Separate user copy actions have separate native event turns. Drain
            // the first OLE notification before transferring ownership again.
            QApplication::processEvents();
            auto *fold = dialog->findChild<QPushButton *>(QStringLiteral("outlineFold"));
            ok &= fold && fold->isEnabled();
            fold->click();
            const bool hidden = term_reading(tab) && !q(term_screen_text(tab)).contains(QStringLiteral("HIDDEN window line"));
            const bool second_visible = q(term_screen_text(tab)).contains(QStringLiteral("SECOND output"));
            ok &= hidden && second_visible;
            auto *fold_menu = chrome::find_menu_action(window.menuBar(), QStringLiteral("view:fold"));
            ok &= fold_menu && fold_menu->isEnabled();
            fold_menu->trigger();
            const bool reopened = q(term_screen_text(tab)).contains(QStringLiteral("HIDDEN window line"));
            ok &= reopened;
            fold->click();
            const auto *copy_item = list->currentItem();
            const auto copy_target = copy_item ? copy_item->data(Qt::UserRole).toULongLong() : 0;
            const bool copy_first = copy_item && copy_item->text().contains(QStringLiteral("FIRST"));
            const auto copy_source = term_command_output(tab, copy_target, false);
            dialog->findChild<QPushButton *>(QStringLiteral("outlineCopyOutput"))->click();
            const bool output_copy = selftest_ui::clipboard_matches(QStringLiteral("FIRST output\nHIDDEN window line\nLAST first output"));
            const QString clipboard_text = output_copy ? QString() : QApplication::clipboard()->text();
            if (!output_copy) {
                const bool owned = QApplication::clipboard()->ownsClipboard();
                int native_owner_present = -1, native_owner_self = -1;
#ifdef Q_OS_WIN
                if (QGuiApplication::platformName() == QLatin1String("windows")) {
                    const HWND owner = GetClipboardOwner();
                    DWORD process = 0;
                    native_owner_present = owner != nullptr;
                    if (owner && GetWindowThreadProcessId(owner, &process))
                        native_owner_self = process == GetCurrentProcessId();
                }
#endif
                QApplication::processEvents();
                std::printf("[commands] copy failure target-first=%d source-available=%d source-exact=%d owned=%d after-events-exact=%d\n",
                    copy_first, copy_source.available,
                    q(copy_source.text) == QStringLiteral("FIRST output\nHIDDEN window line\nLAST first output"), owned,
                    QApplication::clipboard()->text() == QStringLiteral("FIRST output\nHIDDEN window line\nLAST first output"));
                std::printf("[commands] copy native-owner=%d native-self=%d empty=%d previous-command=%d CRLF-only=%d\n",
                    native_owner_present, native_owner_self, clipboard_text.isEmpty(),
                    clipboard_text == QStringLiteral("echo FIRST"),
                    clipboard_text == QStringLiteral("FIRST output\r\nHIDDEN window line\r\nLAST first output"));
            }
            ok &= output_copy;
            std::printf("[commands] copy-command=%d folded=%d other-visible=%d reopen=%d copy-output=%d\n",
                command_copy, hidden, second_visible, reopened, output_copy);
            if (!png_out.empty()) {
                const QString path = QString::fromUtf8(png_out.data(), int(png_out.size()));
                ok &= dialog->grab().save(path);
                ok &= window.grab().save(path + QStringLiteral(".folded.png"));
            }
            dialog->findChild<QPushButton *>(QStringLiteral("outlineJump"))->click();
        });
        outline->trigger();
        ok &= term_reading(tab) && term_uid(tab) == uid && window.active_view() == view;
        std::printf("[commands] outline fold/menu, full copy and same-session jump: %d\n", ok);
        previous->trigger();
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("commandOutline"));
            dialog->findChild<QPushButton *>(QStringLiteral("outlineInsert"))->click();
        });
        outline->trigger();
        ok &= window.page_sendbar(window.current_page())->input()->text() == QStringLiteral("echo SECOND");
        ok &= term_reading(tab); // Filling a draft neither switches mode nor sends.
        std::printf("[commands] sender draft preserves reading mode: %d\n", ok);
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("commandOutline"));
            dialog->findChild<QLineEdit *>(QStringLiteral("outlineFilter"))->setText(QStringLiteral("FIRST"));
            dialog->findChild<QPushButton *>(QStringLiteral("outlineDetachPreview"))->click();
        });
        outline->trigger();
        auto *preview = view->findChild<QDialog *>(QStringLiteral("terminalPreview"));
        if (!preview) { app.exit(1); return; }
        auto *preview_text = preview->findChild<QLabel *>(QStringLiteral("previewText"));
        const bool detached = !preview->isModal() && preview_text->text() ==
            QStringLiteral("FIRST output\nHIDDEN window line\nLAST first output");
        if (!png_out.empty()) ok &= preview->grab().save(
            QString::fromUtf8(png_out.data(), int(png_out.size())) + QStringLiteral(".preview.png"));
        term_clear(tab, 2);
        preview->findChild<QPushButton *>(QStringLiteral("previewRefresh"))->click();
        const bool expired = preview_text->text().isEmpty() &&
            !preview->findChild<QPushButton *>(QStringLiteral("previewCopy"))->isEnabled();
        ok &= detached && expired;
        std::printf("[commands] independent preview outlives outline and rejects expired command: %d\n", detached && expired);
        ok &= selftest_ui::verify_history_scroll(window, view,
            QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_folds(window, view,
            QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_marks(window, view,
            QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_palette(window, view, QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_startup(QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_multiline(QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_senders(QString::fromUtf8(png_out.data(), int(png_out.size())));
        ok &= selftest_ui::verify_completion(QString::fromUtf8(png_out.data(), int(png_out.size())));
        selftest_ui::activate_window(window);
        window.refresh_chrome();
        bool retired_dialog = false;
        QTimer retirement; retirement.setSingleShot(true);
        QObject::connect(&retirement, &QTimer::timeout, &window, [&] {
            auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog || dialog->objectName() != QStringLiteral("commandOutline")) return;
            auto *filter = dialog->findChild<QLineEdit *>(QStringLiteral("outlineFilter"));
            auto *jump = dialog->findChild<QPushButton *>(QStringLiteral("outlineJump"));
            if (!filter || !jump) { dialog->reject(); return; }
            window.close_current_tab();
            filter->setText(QStringLiteral("expired"));
            retired_dialog = !jump->isEnabled();
            dialog->reject();
        });
        retirement.start(0); outline->trigger();
        if (retirement.isActive()) retirement.stop();
        ok &= retired_dialog;
        std::printf("[commands] dialog handles retired pane: %d\n", retired_dialog);
        QApplication::clipboard()->clear();
        // Earlier fold/mark fixtures resize this window. Restore room for the
        // isolated cwd and command before checking exact real-shell output.
        window.resize(1400, 800);
        QApplication::processEvents();
        SessCfg draft = sess_form_get("", 0);
#ifdef Q_OS_WIN
        draft.local_shell = "pwsh";
        const QByteArray shell_command("Write-Output B2A_REAL_SHELL\r");
        const QString expected_title = QStringLiteral("Write-Output B2A_REAL_SHELL");
        const QString prompt = QStringLiteral("PS ");
        // A real prompt may draw its text before restoring the input cursor.
        // Keep the production hook, but expose that window once in this fixture.
        const QByteArray archive_fixture(
            "$global:ttB01ArchivePrompt=$function:prompt; $global:ttB01ArchiveOnce=$true; "
            "function global:prompt { if ($global:ttB01ArchiveOnce) { $global:ttB01ArchiveOnce=$false; "
            "$p=& $global:ttB01ArchivePrompt; [Console]::Write($p); [Console]::Write(([char]27+'7'+[char]27+'[?25h'+[char]27+'[1A!')); "
            "Start-Sleep -Milliseconds 250; [Console]::Write(([char]27+'8'+[char]27+'[1G')); return $p }; "
            "& $global:ttB01ArchivePrompt }; Clear-Host; Write-Output ('B01_'+'ARCHIVE_READY')\r");
#else
        draft.local_shell = "bash";
        const QByteArray shell_command("printf 'B2A_REAL_SHELL\\n'\r");
        const QString expected_title = QStringLiteral("printf 'B2A_REAL_SHELL\\n'");
        const QString prompt = QStringLiteral("$");
        const QByteArray archive_fixture;
#endif
        const uint32_t shell = sess_open_draft(draft);
        ok &= shell != UINT32_MAX;
        if (shell == UINT32_MAX) { app.exit(1); return; }
        window.add_tab_with(shell);
        auto *poll = new QTimer(&window);
        QObject::connect(poll, &QTimer::timeout, &window, [&, poll, shell, shell_command, expected_title, prompt, archive_fixture,
                         ticks = 0, sent = false, fixture_sent = false, waited_for_cursor = false]() mutable {
            ++ticks;
            const QString raw_screen = q(term_screen_text(shell));
            const QString screen = raw_screen.trimmed();
            const auto state = term_stats(shell);
            const bool cursor_on_last = int(state.cursor_row) == raw_screen.count(QLatin1Char('\n'));
#ifdef Q_OS_WIN
            const bool prompt_ready = screen.contains(prompt) && screen.endsWith(QLatin1Char('>'));
#else
            const bool prompt_ready = screen.contains(prompt) && screen.endsWith(QLatin1Char('$'));
#endif
            if (!fixture_sent && !archive_fixture.isEmpty() && prompt_ready && cursor_on_last) {
                term_write(shell, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(archive_fixture.data()), size_t(archive_fixture.size())));
                fixture_sent = true;
                return;
            }
            const bool fixture_ready = archive_fixture.isEmpty() || screen.contains(QStringLiteral("B01_ARCHIVE_READY"));
            if (!sent && prompt_ready && fixture_ready && !cursor_on_last) waited_for_cursor = true;
            // Prompt text can already be visible while a status redraw still
            // owns the cursor. Submit only on the final physical prompt row.
            if (!sent && prompt_ready && fixture_ready && cursor_on_last) {
                const QString cursor_line = raw_screen.section(QLatin1Char('\n'), int(state.cursor_row), int(state.cursor_row));
                std::printf("[commands] prompt-admission cursor-on-last=%d cursor-line-prompt=%d cursor-col=%u waited=%d\n",
                    cursor_on_last, cursor_line.contains(prompt), state.cursor_col, waited_for_cursor);
                std::printf("[commands] before real send scroll=%u cols=%u row=%u reading=%d\n",
                    term_scroll_offset(shell), term_stats(shell).cols, term_stats(shell).cursor_row, term_reading(shell));
                term_write(shell, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(shell_command.data()), size_t(shell_command.size())));
                bool admitted = false;
                for (const auto &row : term_commands(shell)) admitted |= q(row.title) == expected_title;
                std::printf("[commands] real shell submitted block=%d cursor=%u\n", admitted, term_stats(shell).cursor_col);
                sent = true;
            }
            const auto blocks = term_commands(shell);
            bool found = false;
            bool matched = false, finished = false, output_contains_marker = false, output_contains_echo = false;
            size_t output_bytes = 0;
            for (const auto &block : blocks) {
                if (q(block.title) == expected_title && block.finished) {
                    matched = true; finished = block.finished;
                    const auto output = term_command_output(shell, block.id, false);
                    found = output.available && q(output.text).trimmed() == QStringLiteral("B2A_REAL_SHELL");
                    output_bytes = output.text.size();
                    output_contains_marker = q(output.text).contains(QStringLiteral("B2A_REAL_SHELL"));
                    output_contains_echo = q(output.text).contains(QStringLiteral("Write-Output"));
                }
            }
            if (found || ticks >= 400) {
                poll->stop(); ok &= found;
                if (!found) {
                    std::printf("[commands] prompt state fixture-sent=%d fixture-ready=%d prompt-ready=%d cursor-last=%d row=%u last-row=%d col=%u status=%u\n",
                        fixture_sent, fixture_ready, prompt_ready, cursor_on_last, state.cursor_row,
                        int(raw_screen.count(QLatin1Char('\n'))), state.cursor_col, unsigned(term_status_kind(shell)));
                    const int last = int(raw_screen.count(QLatin1Char('\n')));
                    for (int row = qMax(0, last - 3); row <= last; ++row) {
                        const auto line = raw_screen.section(QLatin1Char('\n'), row, row);
                        std::printf("[commands] row-shape row=%d length=%d prompt=%d delimiter=%d fixture-head=%d fixture-tail=%d bang=%d\n",
                            row, int(line.size()), line.contains(prompt), line.trimmed().endsWith(QLatin1Char('>')),
                            line.contains(QStringLiteral("B01_")), line.contains(QStringLiteral("ARCHIVE_READY")),
                            line.contains(QLatin1Char('!')));
                    }
                }
                if (!found) std::printf("[commands] real shell failure sent=%d blocks=%zu matched=%d finished=%d output-bytes=%zu marker=%d echo=%d cols=%u reading=%d screen-marker=%d\n",
                    sent, blocks.size(), matched, finished, output_bytes, output_contains_marker, output_contains_echo,
                    term_stats(shell).cols, term_reading(shell), q(term_screen_text(shell)).contains(QStringLiteral("B2A_REAL_SHELL")));
                if (!found) for (const auto &row : blocks) std::printf("[commands] retained block startup=%d title-expected=%d finished=%d line=%llu\n",
                    row.startup, q(row.title) == expected_title, row.finished, static_cast<unsigned long long>(row.line));
                std::printf("[commands] real shell submission/output/prompt boundary: %d\n", found);
                if (found && !png_out.empty()) ok &= window.grab().save(
                    QString::fromUtf8(png_out.data(), int(png_out.size())) + QStringLiteral(".shell.png"));
                verdict = ok ? 0 : 1; app.exit(verdict);
            }
        });
        poll->start(25);
    });
    // Startup, multiline and auxiliary shell admissions each have their own bound.
    QTimer::singleShot(75000, &app, [&] { app.exit(2); });
    app.exec();
    return verdict;
}
