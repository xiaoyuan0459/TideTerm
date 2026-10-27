// UI acceptance must not reach SendBar::returnPressed. Real shell output owns send proof.
#include "cpp/chrome/selftest_completion.h"
#include "cpp/chrome/selftest_command_templates.h"
#include "cpp/chrome/selftest_environment.h"
#include "cpp/chrome/selftest_programs.h"
#include "cpp/chrome/selftest_shell_report.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QElapsedTimer>
#include <QInputMethodEvent>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QTemporaryDir>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <cstdio>
#include <functional>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &predicate, int ms = 5000) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); QApplication::processEvents(); if (predicate()) return true; QThread::msleep(10); }
    while (timer.elapsed() < ms);
    return false;
}
void key(QWidget *widget, int code, Qt::KeyboardModifiers mods = Qt::NoModifier, const QString &text = {}) {
    QKeyEvent press(QEvent::KeyPress, code, mods, text); QApplication::sendEvent(widget, &press);
}
void type(QLineEdit *edit, const QString &text) {
    edit->selectAll(); key(edit, Qt::Key_Backspace); key(edit, 0, Qt::NoModifier, text);
}
}

bool selftest_ui::verify_completion(const QString &png) {
    std::printf("[completion] begin\n");
    const QString root = QDir(QCoreApplication::applicationDirPath()).absoluteFilePath(QStringLiteral("../fixtures"));
    QDir().mkpath(root); QTemporaryDir fixture(root + QStringLiteral("/completion-XXXXXX"));
    if (!fixture.isValid()) return false;
    auto cfg = sess_form_get("", 0);
    cfg.local_cwd = rust::String(fixture.path().toUtf8().toStdString());
#ifdef Q_OS_WIN
    // Default self-test launch isolates profiles and disables persistent shell history.
    cfg.local_shell = "pwsh"; cfg.local_args = "";
    const QString command = QStringLiteral("Write-Output ('B3C_'+'ONCE')");
#else
    cfg.local_shell = "bash"; cfg.local_args = "";
    const QString command = QStringLiteral("printf '%s%s\\n' B3C_ ONCE");
#endif
    const auto tab = sess_open_draft(cfg);
    if (tab == UINT32_MAX) return false;
    MainWindow window(false); window.add_tab_with(tab); window.show();
    if (!activate_window(window)) return false;
    auto *bar = window.page_sendbar(window.current_page());
    if (!bar || !wait_for([&] {
        const QString screen = q(term_screen_text(tab)).trimmed();
        return completion_query(tab, "").context.ready &&
            (screen.endsWith(QLatin1Char('>')) || screen.endsWith(QLatin1Char('$')) || screen.endsWith(QLatin1Char('#')));
    })) return false;
    auto *input = bar->input();
    input->setFocus();
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    auto *manage = bar->findChild<QToolButton *>(QStringLiteral("sendManageCommands"));
    if (!popup || !manage) return false;
    bool ok = command_catalog().entries.empty();
    auto edit = [&](const std::function<void(QDialog *)> &action) {
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog || dialog->objectName() != QStringLiteral("userCommandsDialog")) { ok = false; return; }
            action(dialog);
            if (dialog->isVisible()) { ok = false; dialog->reject(); }
        });
        manage->click();
    };
    auto fill = [&](QDialog *dialog) {
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandAdd"))->click();
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandGroup")), QStringLiteral("Demo"));
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandName")), QStringLiteral("Once"));
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandText")), command);
    };
    edit([&](QDialog *dialog) { fill(dialog); dialog->reject(); });
    ok &= command_catalog().entries.empty();
    edit([&](QDialog *dialog) { fill(dialog); dialog->findChild<QPushButton *>(QStringLiteral("userCommandSave"))->click(); });
    auto saved = command_catalog();
    const bool created = saved.entries.size() == 1 && q(saved.entries[0].command) == command;
    ok &= created; if (!created) return false;
    edit([&](QDialog *dialog) {
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandName")), QStringLiteral("Changed")); dialog->reject();
    });
    ok &= q(command_catalog().entries[0].name) == QStringLiteral("Once");
    edit([&](QDialog *dialog) {
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandName")), QStringLiteral("Run once"));
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandSave"))->click();
    });
    ok &= q(command_catalog().entries[0].name) == QStringLiteral("Run once");
    bool deleted_fields = false;
    edit([&](QDialog *dialog) {
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandDelete"))->click();
        deleted_fields = !dialog->findChild<QLineEdit *>(QStringLiteral("userCommandText"))->isEnabled()
            && dialog->findChild<QLineEdit *>(QStringLiteral("userCommandText"))->text().isEmpty();
        dialog->reject();
    });
    ok &= deleted_fields;
    std::printf("[completion] deleting last entry clears/disables fields: %d\n", deleted_fields);
    std::printf("[completion] grouped create/edit/save/cancel: %d\n", ok);
    if (!activate_window(window)) return false;
    input->setFocus(); type(input, QStringLiteral("Demo"));
    const bool automatic = popup->isVisible() && popup->count() == 1;
    ok &= automatic;
    std::printf("[completion] automatic suggestions focus=%d visible=%d count=%d: %d\n", input->hasFocus(), popup->isVisible(), popup->count(), automatic);
    if (!automatic) return false;
    key(input, Qt::Key_Escape); ok &= !popup->isVisible() && input->text() == QStringLiteral("Demo");
    key(input, Qt::Key_Space, Qt::ControlModifier);
    key(input, Qt::Key_Return);
    const bool accepted = input->text() == command && !popup->isVisible() && bar->sent_count() == 0;
    ok &= accepted;
    std::printf("[completion] Enter fills draft without sending: %d\n", accepted);
    if (!accepted) return false;
    QKeyEvent repeat(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true, 1);
    QApplication::sendEvent(input, &repeat);
    const bool repeat_silent = bar->sent_count() == 0;
    std::printf("[completion] holding accept key never sends: %d\n", repeat_silent);
    if (!repeat_silent) return false;
    ok &= !q(term_screen_text(tab)).contains(QStringLiteral("B3C_ONCE"));
    input->setText(QStringLiteral("Demo")); key(input, Qt::Key_Space, Qt::ControlModifier); key(input, Qt::Key_Tab);
    ok &= input->text() == command && bar->sent_count() == 0;
    // While composing, Enter belongs to the input method and suggestions are hidden.
    key(input, Qt::Key_Space, Qt::ControlModifier);
    QInputMethodEvent preedit(QStringLiteral("中"), {}); QApplication::sendEvent(input, &preedit);
    ok &= !popup->isVisible();
    QInputMethodEvent cancel; QApplication::sendEvent(input, &cancel);
    input->setText(QStringLiteral("Demo")); key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!popup->isVisible()) return false;
    term_set_reading(tab, true); key(input, Qt::Key_Return);
    ok &= input->text() == QStringLiteral("Demo") && bar->sent_count() == 0;
    term_set_reading(tab, false);
    key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!popup->isVisible()) return false;
    bar->set_hex_mode(true); key(input, Qt::Key_Return);
    ok &= input->text() == QStringLiteral("Demo") && bar->sent_count() == 0;
    bar->set_hex_mode(false);
    key(input, Qt::Key_Space, Qt::ControlModifier); key(input, Qt::Key_Return);
    if (!png.isEmpty()) ok &= window.grab().save(png + QStringLiteral(".completion-draft.png"));
    std::printf("[completion] Enter/Tab/Escape/IME/reading/Hex never send: %d\n", ok);
    // A second, explicit Enter sends once through the production writer.
    key(input, Qt::Key_Return);
    const bool executed = wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("B3C_ONCE")); });
    ok &= executed && bar->sent_count() == 1 && q(term_screen_text(tab)).count(QStringLiteral("B3C_ONCE")) == 1;
    input->setText(QStringLiteral("B3C_")); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
    const auto history = completion_query(tab, "B3C_");
    ok &= !history.items.empty() && history.items[0].kind == 0 && popup->isVisible() && popup->count() == 1;
    if (!png.isEmpty()) {
        ok &= window.grab().save(png + QStringLiteral(".completion-history.png"));
        ok &= popup->grab().save(png + QStringLiteral(".completion-popup.png"));
    }
    key(input, Qt::Key_Escape);
    std::printf("[completion] explicit real shell send once/history dedup: %d\n", ok);
    edit([&](QDialog *dialog) {
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandDelete"))->click();
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandSave"))->click();
    });
    ok &= command_catalog().entries.empty();
    std::printf("[completion] explicit delete/save: %d\n", ok);
    ok &= verify_environment(window, bar, tab, fixture.path(), png);
    ok &= verify_programs(window, bar, tab, fixture.path(), png);
    ok &= verify_shell_report(window, bar, tab, png);
    ok &= verify_git_refs(window, bar, tab, fixture.path(), png);
    ok &= verify_docker(window, bar, tab, png);
#ifdef Q_OS_WIN
    ok &= verify_kube(window, bar, tab, png);
#endif
    ok &= verify_command_templates(window, bar, tab, png);
    ok &= verify_remote_environment(png);
    return ok;
}
