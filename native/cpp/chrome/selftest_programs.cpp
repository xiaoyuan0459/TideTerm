#include "cpp/chrome/selftest_programs.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/send_programs.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QElapsedTimer>
#include <QKeyEvent>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <cstdio>
#include <functional>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
void key(QWidget *widget, int code, Qt::KeyboardModifiers modifiers = Qt::NoModifier) {
    QKeyEvent event(QEvent::KeyPress, code, modifiers); QApplication::sendEvent(widget, &event);
}
bool wait_for(const std::function<bool()> &predicate) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); QApplication::processEvents(); if (predicate()) return true; QThread::msleep(10); } while (timer.elapsed() < 6000);
    return false;
}
int row(QListWidget *popup, int kind) {
    for (int i = 0; i < popup->count(); ++i) if (popup->item(i)->data(Qt::UserRole + 1).toInt() == kind) return i;
    return -1;
}
bool edit(MainWindow &window, const std::function<void()> &trigger, const QString &text, bool save, bool invalid = false) {
    bool ok = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != QStringLiteral("programDirectoriesDialog")) return;
        auto *lines = dialog->findChild<QPlainTextEdit *>(QStringLiteral("programDirectoryLines"));
        auto *buttons = dialog->findChild<QDialogButtonBox *>();
        if (!lines || !buttons) { dialog->reject(); return; }
        lines->setPlainText(text);
        if (save) buttons->button(QDialogButtonBox::Save)->click();
        if (invalid) {
            auto *error = dialog->findChild<QLabel *>(QStringLiteral("programDirectoriesError"));
            ok = dialog->isVisible() && error && !error->text().isEmpty();
        } else ok = save ? !dialog->isVisible() : dialog->isVisible();
        if (dialog->isVisible()) buttons->button(QDialogButtonBox::Cancel)->click();
    });
    trigger(); return ok;
}
}

bool selftest_ui::verify_programs(MainWindow &window, SendBar *bar, uint32_t tab, const QString &directory, const QString &png) {
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendProgramDirectories"));
    auto *configure = bar->findChild<QAction *>(QStringLiteral("sendEditProgramDirectories"));
    auto *syntax = bar->findChild<QComboBox *>(QStringLiteral("sendPathSyntax"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    auto *input = bar->input();
    if (!button || !configure || !syntax || !popup || !activate_window(window)) return false;
    const auto sent = bar->sent_count();
    input->setText(QStringLiteral("TIDE")); input->setFocus();
    bool ok = true;
    const auto expect = [&ok](const char *what, bool value) {
        if (!value) std::printf("[programs] failed step: %s\n", what);
        ok &= value;
    };
    expect("starts disabled", !button->isChecked());
    // Destroy the actual settings owner during its modal loop. It must reject
    // the editor and never call back into a retired sender.
    auto *temporary = new QWidget;
    const QPointer<QWidget> retired(temporary);
    auto *temporary_row = new QHBoxLayout(temporary);
    bool refreshed = false;
    auto *settings = new chrome::ProgramSettings(temporary, temporary_row, [tab] { return tab; }, [] {},
        [&] { refreshed = true; }, [](uint8_t) { return QString(); });
    QTimer::singleShot(0, &window, [temporary] { delete temporary; });
    settings->button()->click();
    expect("retired settings owner", !retired && !refreshed && QApplication::activeModalWidget() == nullptr);
    expect("invalid save stays in editor", edit(window, [&] { button->click(); }, QStringLiteral("relative-path"), true, true));
    expect("invalid save leaves state", !button->isChecked() && input->text() == QStringLiteral("TIDE") && bar->sent_count() == sent);
    QString probe = qEnvironmentVariable("TIDETERM_TEST_PROGRAM_DIR");
    const bool run_probe = !probe.isEmpty();
    if (run_probe) {
        probe = QDir(probe).canonicalPath();
        if (probe.isEmpty() || !probe.startsWith(QDir(QDir::tempPath()).canonicalPath() + QLatin1Char('/'), Qt::CaseInsensitive)) return false;
    } else probe = directory;
    expect("save probe directory", edit(window, [&] { button->click(); }, probe, true));
    expect("probe enabled with syntax", button->isChecked() && syntax->isEnabled());
    expect("configure action reopens editor", edit(window, [&] { configure->trigger(); }, directory + QStringLiteral("/missing"), false));
    if (run_probe) {
        syntax->setCurrentIndex(2); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
        if (!wait_for([&] { return popup->isVisible() && row(popup, 5) >= 0; })) {
            std::printf("[programs] no local candidate: focus=%d checked=%d visible=%d rows=%d syntax=%d\n", input->hasFocus(), button->isChecked(), popup->isVisible(), popup->count(), syntax->currentIndex());
            return false;
        }
        const auto command = popup->item(row(popup, 5))->data(Qt::UserRole).toString();
        expect("PowerShell literal program", command.startsWith(QStringLiteral("& '")) && command.contains(QStringLiteral("TIDE_PROBE")));
        if (!png.isEmpty()) {
            expect("window screenshot", window.grab().save(png + QStringLiteral(".programs.png")));
            expect("popup screenshot", popup->grab().save(png + QStringLiteral(".programs-popup.png")));
        }
        syntax->setCurrentIndex(0);
        const bool retired_program = row(popup, 5) < 0;
        key(input, Qt::Key_Return);
        // Other sources may fill a different draft; the retired program cannot be accepted or sent.
        const bool retired_and_kept = retired_program && input->text() != command && bar->sent_count() == sent;
        if (!retired_and_kept) std::printf("[programs] syntax retire failed: retired=%d text=[%s] command=[%s] sent=%u/%u\n",
            retired_program, input->text().toUtf8().constData(), command.toUtf8().constData(), bar->sent_count(), sent);
        expect("syntax change retires program", retired_and_kept);
        input->setText(QStringLiteral("TIDE")); input->setFocus();
        syntax->setCurrentIndex(2);
        // A late terminal notification changes the completion context after the
        // program scan starts. The sender must re-query the same draft instead
        // of silently closing the list.
        const QByteArray late("\x1b]777;tideterm-command;programs-late-event\x07");
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(late.constData()), size_t(late.size())));
        if (!wait_for([&] { return row(popup, 5) >= 0; })) {
            std::printf("[programs] mode restore failed: focus=%d checked=%d rows=%d ready=%d history=%llu\n", input->hasFocus(), button->isChecked(), popup->count(), completion_context(tab).ready, static_cast<unsigned long long>(completion_context(tab).history_revision));
            return false;
        }
        popup->setCurrentRow(row(popup, 5)); key(input, Qt::Key_Tab);
        expect("Tab fills program draft", input->text() == command && bar->sent_count() == sent);
        QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true, 1);
        QApplication::sendEvent(input, &held); expect("held accept never sends", bar->sent_count() == sent);
        key(input, Qt::Key_Return);
        // Output can precede the shell's next prompt/history notification.
        // Query only after that boundary so the test does not request a stale snapshot.
        expect("program executes once", wait_for([&] {
            const auto screen = q(term_screen_text(tab)).trimmed();
            return screen.count(QStringLiteral("PROGRAM_DIRECTORY_ONCE")) == 1
                && (screen.endsWith(QLatin1Char('>')) || screen.endsWith(QLatin1Char('$')) || screen.endsWith(QLatin1Char('#')));
        }));
        expect("one program send", bar->sent_count() == sent + 1);
        input->setText(QStringLiteral("TIDE")); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
        if (!wait_for([&] {
            for (int i = 0; i < popup->count(); ++i)
                if (popup->item(i)->data(Qt::UserRole).toString() == command) return popup->isVisible();
            return false;
        })) {
            std::printf("[programs] post-send refresh failed: focus=%d checked=%d rows=%d ready=%d\n", input->hasFocus(), button->isChecked(), popup->count(), completion_context(tab).ready);
            return false;
        }
        expect("reconfigure without candidates", edit(window, [&] { configure->trigger(); }, directory, true));
        expect("directory change retires candidates", wait_for([&] { return row(popup, 5) < 0; }));
        expect("no extra send", bar->sent_count() == sent + 1);
    }
    button->click(); key(input, Qt::Key_Escape);
    expect("disable clears syntax", !button->isChecked() && !syntax->isEnabled());
    std::printf("[programs] configure/invalid/cancel/disable=%d probe=%d explicit-once=%d\n", ok, run_probe, ok);
    return ok;
}

bool selftest_ui::verify_ssh_programs(MainWindow &window, SendBar *bar, uint32_t, const QString &png) {
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendProgramDirectories"));
    auto *syntax = bar->findChild<QComboBox *>(QStringLiteral("sendPathSyntax"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !syntax || !popup || !activate_window(window)) return false;
    auto *input = bar->input(); const auto sent = bar->sent_count();
    input->setText(QStringLiteral("TIDE")); input->setFocus();
    bool ok = !button->isChecked() && edit(window, [&] { button->click(); }, QStringLiteral("/programs"), true);
    syntax->setCurrentIndex(1); key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!wait_for([&] { return popup->isVisible() && row(popup, 6) >= 0; })) {
        std::printf("[programs-ssh] no remote candidate: focus=%d checked=%d visible=%d rows=%d syntax=%d\n", input->hasFocus(), button->isChecked(), popup->isVisible(), popup->count(), syntax->currentIndex());
        return false;
    }
    const auto command = popup->item(row(popup, 6))->data(Qt::UserRole).toString();
    ok &= command.contains(QStringLiteral("/programs/TIDE"));
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".ssh-programs-popup.png"));
    popup->setCurrentRow(row(popup, 6)); key(input, Qt::Key_Tab);
    ok &= input->text() == command && bar->sent_count() == sent;
    button->click(); key(input, Qt::Key_Escape);
    std::printf("[programs-ssh] authenticated metadata/source/explicit draft/disable: %d\n", ok);
    return ok;
}
