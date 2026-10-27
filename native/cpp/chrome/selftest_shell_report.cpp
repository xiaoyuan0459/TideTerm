#include "cpp/chrome/selftest_shell_report.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/send_shell_report.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QElapsedTimer>
#include <QLineEdit>
#include <QListWidget>
#include <QKeyEvent>
#include <QHBoxLayout>
#include <QPointer>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &f) {
    QElapsedTimer time; time.start();
    do { term_pump(); QApplication::processEvents(); if (f()) return true; QThread::msleep(10); } while (time.elapsed() < 12000);
    return false;
}
}
namespace {
bool verify_cached_suggestions(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
#ifdef Q_OS_WIN
    const uint8_t dialect = 2;
#else
    const uint8_t dialect = 1;
#endif
    auto *input = bar->input();
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!popup) return false;
    const auto sent = bar->sent_count();
    auto key = [&](int code, Qt::KeyboardModifiers mods = Qt::NoModifier) {
        QKeyEvent event(QEvent::KeyPress, code, mods); QApplication::sendEvent(input, &event);
    };
    auto query = [&](const char *name) {
        const auto preview = shell_resolution_prepare(tab, dialect, rust::Str(name));
        const bool ready = preview.state == 1 && shell_report_send(tab, preview.token) == 2
            && wait_for([&] { return shell_report_poll(tab).state == 3; });
        std::printf("[shell-suggestions] explicit lookup ready=%d state=%u\n", ready, unsigned(shell_report_poll(tab).state));
        return ready;
    };
    auto show = [&] {
        if (!selftest_ui::activate_window(window)) return false;
        input->setFocus(); input->setText(QStringLiteral("Tide_")); key(Qt::Key_Space, Qt::ControlModifier);
        for (int row = 0; row < popup->count(); ++row) {
            if (popup->item(row)->data(Qt::UserRole + 1).toInt() == 7) {
                popup->setCurrentRow(row);
                std::printf("[shell-suggestions] focus=%d visible=%d rows=%d\n", input->hasFocus(), popup->isVisible(), popup->count());
                return popup->isVisible();
            }
        }
        std::printf("[shell-suggestions] no Shell row focus=%d visible=%d rows=%d state=%u\n", input->hasFocus(), popup->isVisible(), popup->count(), unsigned(shell_report_poll(tab).state));
        return false;
    };
    if (!query("Tide_Query") || !show()) return false;
    shell_report_clear(tab); key(Qt::Key_Return);
    const bool stale = !popup->isVisible() && input->text() == QStringLiteral("Tide_") && bar->sent_count() == sent;
    if (!query("Tide_Query") || !show()) return false;
    const auto receipt = shell_report_poll(tab).token;
    const bool source = popup->currentItem()->text().contains(QStringLiteral("Shell"))
        && popup->currentItem()->toolTip().contains(QStringLiteral("PowerShell")) == (dialect == 2);
    bool ok = stale && source;
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".shell-suggestions.png"));
    key(Qt::Key_Tab);
    const bool filled = !popup->isVisible() && input->text() == QStringLiteral("Tide_Query")
        && shell_resolution_draft(tab, receipt).empty() && shell_report_poll(tab).state == 0;
    QKeyEvent repeat(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true);
    QApplication::sendEvent(input, &repeat);
    // Sender dispatch owns this UI assertion. SSH byte counters independently
    // prove zero target input; visible screen text can legitimately scroll away.
    const bool held = bar->sent_count() == sent;
    if (!query("Tide_Query") || !show()) return false;
    shell_report_clear(tab);
    const bool retired = wait_for([&] { return !popup->isVisible(); });
    if (!query("Tide_CacheMissing")) return false;
    if (!selftest_ui::activate_window(window)) return false;
    input->setFocus(); input->setText(QStringLiteral("Tide_CacheMissing")); key(Qt::Key_Space, Qt::ControlModifier);
    bool missing = false;
    for (int row = 0; row < popup->count(); ++row) {
        const auto *item = popup->item(row);
        missing |= popup->isVisible() && item->flags() == Qt::NoItemFlags
            && item->text().contains(QStringLiteral("Shell")) && item->text().contains(QStringLiteral("Tide_CacheMissing"));
    }
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".shell-missing.png"));
    key(Qt::Key_Escape);
    const bool silent = input->text() == QStringLiteral("Tide_CacheMissing") && bar->sent_count() == sent;
    ok &= filled && held && retired && missing && silent;
    shell_report_clear(tab); input->clear();
    std::printf("[shell-suggestions] source=%d stale=%d consume=%d held=%d timer=%d missing=%d silent=%d\n", source, stale, filled, held, retired, missing, silent);
    return ok;
}
bool verify_lookup(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
#ifdef Q_OS_WIN
    const QByteArray define("function global:Tide_Query { [Console]::WriteLine(('SHELL_LOOKUP_'+'ONCE')) }; [Console]::WriteLine(('LOOKUP_'+'READY'))\r");
    const int dialect = 2;
#else
    const QByteArray define("Tide_Query() { builtin printf 'SHELL_LOOKUP_%s\\n' ONCE; }; builtin printf 'LOOKUP_%s\\n' READY\r");
    const int dialect = 1;
#endif
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(define.constData()), size_t(define.size())));
    if (!wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("LOOKUP_READY")); })) return false;
    auto *input = bar->input(); const auto original = input->text(); const auto sent = bar->sent_count();
    bool ok = true;
    QTimer watchdog; watchdog.setSingleShot(true);
    QObject::connect(&watchdog, &QTimer::timeout, &window, [&] {
        ok = false; if (auto *active = dynamic_cast<QDialog *>(QApplication::activeModalWidget())) active->reject();
    });
    watchdog.start(20000);
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog) { ok = false; return; }
        auto *mode = dialog->findChild<QComboBox *>(QStringLiteral("shellReportMode"));
        auto *syntax = dialog->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
        auto *name = dialog->findChild<QLineEdit *>(QStringLiteral("shellLookupName"));
        auto *refresh = dialog->findChild<QPushButton *>(QStringLiteral("shellReportRefresh"));
        auto *confirm = dialog->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
        auto *send = dialog->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
        auto *apply = dialog->findChild<QPushButton *>(QStringLiteral("shellLookupApply"));
        if (!mode || !syntax || !name || !refresh || !confirm || !send || !apply) { ok = false; dialog->reject(); return; }
        mode->setCurrentIndex(1); syntax->setCurrentIndex(dialect);
        name->setText(QStringLiteral("bad name")); refresh->click();
        ok &= shell_report_poll(tab).state == 9 && !send->isEnabled() && !apply->isEnabled();
        name->setText(QStringLiteral("Tide_Query"));
        ok &= shell_report_poll(tab).state == 0 && input->text() == original;
        refresh->click(); ok &= shell_report_poll(tab).state == 1 && !send->isEnabled();
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".shell-lookup-preview.png"));
        confirm->setChecked(true); send->click();
        ok &= wait_for([&] { return shell_report_poll(tab).state == 3 && apply->isEnabled(); });
        const auto first = shell_report_poll(tab);
        ok &= first.kind == 2 && input->text() == original && bar->sent_count() == sent;
        name->setText(QStringLiteral("Tide_Other"));
        ok &= shell_report_poll(tab).state == 0 && !apply->isEnabled() && shell_resolution_draft(tab, first.token).empty();
        name->setText(QStringLiteral("Tide_Query")); refresh->click(); confirm->setChecked(true); send->click();
        ok &= wait_for([&] { return shell_report_poll(tab).state == 3 && apply->isEnabled(); });
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".shell-lookup-ready.png"));
        apply->click();
    });
    bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"))->click(); watchdog.stop();
    ok &= input->text() == QStringLiteral("Tide_Query") && bar->sent_count() == sent;
    ok &= !q(term_screen_text(tab)).contains(QStringLiteral("SHELL_LOOKUP_ONCE"));
    QKeyEvent repeated(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true); QApplication::sendEvent(input, &repeated);
    QKeyEvent released(QEvent::KeyRelease, Qt::Key_Return, Qt::NoModifier); QApplication::sendEvent(input, &released);
    ok &= bar->sent_count() == sent;
    QKeyEvent press(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier); QApplication::sendEvent(input, &press);
    ok &= wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("SHELL_LOOKUP_ONCE")); });
    ok &= bar->sent_count() == sent + 1 && q(term_screen_text(tab)).count(QStringLiteral("SHELL_LOOKUP_ONCE")) == 1;
    std::printf("[shell-lookup] invalid/edit/preview/no-exec/fresh-draft/held-key/explicit-once: %d\n", ok);
    return ok;
}
}

bool selftest_ui::verify_shell_report(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    if (!button) return false;
    // Synthetic data only: no real environment can enter logs or screenshots.
#ifdef Q_OS_WIN
    const QByteArray assignment("$global:ttReportTestPath=$env:PATH; $env:PATH='C:\\SHELL_REPORT_ONLY'; [Console]::WriteLine(('REPORT_'+'READY'))\r");
    const QByteArray restore("$env:PATH=$global:ttReportTestPath; Remove-Variable ttReportTestPath -Scope Global; [Console]::WriteLine(('REPORT_'+'RESTORED'))\r");
    const int dialect = 2;
    const QString expected = QStringLiteral("C:\\SHELL_REPORT_ONLY");
#else
    const QByteArray assignment("TIDETERM_REPORT_TEST_PATH=$PATH; PATH=/SHELL_REPORT_ONLY; builtin printf 'REPORT_%s\\n' READY\r");
    const QByteArray restore("PATH=$TIDETERM_REPORT_TEST_PATH; unset TIDETERM_REPORT_TEST_PATH; builtin printf 'REPORT_%s\\n' RESTORED\r");
    const int dialect = 1;
    const QString expected = QStringLiteral("/SHELL_REPORT_ONLY");
#endif
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(assignment.constData()), size_t(assignment.size())));
    if (!wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("REPORT_READY")); })) return false;
    const auto draft = bar->input()->text(); const auto sent = bar->sent_count();
    bool ok = shell_report_poll(tab).state == 0;
    auto *temporary = new QWidget;
    const QPointer<QWidget> retired(temporary);
    auto *row = new QHBoxLayout(temporary);
    chrome::install_shell_report(temporary, row, [tab] { return tab; }, [] {}, [] { return true; }, new QLineEdit(temporary));
    auto *temporary_button = temporary->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    QTimer::singleShot(0, &window, [temporary] { delete temporary; });
    temporary_button->click();
    ok &= !retired && QApplication::activeModalWidget() == nullptr;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != QStringLiteral("shellReportDialog")) return;
        auto *syntax = dialog->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
        auto *preview = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPreview"));
        auto *confirm = dialog->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
        auto *send = dialog->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
        auto *clear = dialog->findChild<QPushButton *>(QStringLiteral("shellReportClear"));
        auto *refresh = dialog->findChild<QPushButton *>(QStringLiteral("shellReportRefresh"));
        auto *path = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
        if (!syntax || !preview || !confirm || !send || !clear || !refresh || !path) { ok = false; dialog->reject(); return; }
        ok &= !send->isEnabled() && preview->toPlainText().isEmpty();
        syntax->setCurrentIndex(dialect);
        ok &= !preview->toPlainText().isEmpty() && !send->isEnabled() && shell_report_poll(tab).state == 1;
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".shell-report-preview.png"));
        clear->click(); ok &= shell_report_poll(tab).state == 0;
        refresh->click();
        bar->set_hex_mode(true); confirm->setChecked(true); ok &= !send->isEnabled();
        bar->set_hex_mode(false); confirm->setChecked(false);
        confirm->setChecked(true); ok &= send->isEnabled(); send->click();
        ok &= !send->isEnabled();
        ok &= wait_for([&] { return shell_report_poll(tab).state == 3; });
        ok &= wait_for([&] { return path->toPlainText().contains(expected); });
        const auto report = shell_report_poll(tab);
        ok &= q(report.path) == expected && bar->input()->text() == draft && bar->sent_count() == sent;
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".shell-report-ready.png"));
        clear->click(); ok &= shell_report_poll(tab).state == 0;
        syntax->setCurrentIndex(0); syntax->setCurrentIndex(dialect);
        dialog->reject();
    });
    button->click();
    ok &= shell_report_poll(tab).state == 0 && bar->input()->text() == draft && bar->sent_count() == sent;
    ok &= verify_lookup(window, bar, tab, png);
    ok &= verify_cached_suggestions(window, bar, tab, png);
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(restore.constData()), size_t(restore.size())));
    ok &= wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("REPORT_RESTORED")); });
    ok &= verify_names(window, bar, tab, png);
    std::printf("[shell-report] preview/confirmation/real PATH/clear/cancel/draft: %d\n", ok);
    return ok;
}
