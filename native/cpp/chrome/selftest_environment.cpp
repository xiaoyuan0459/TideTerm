#include "cpp/chrome/selftest_environment.h"
#include "cpp/chrome/selftest_programs.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QComboBox>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QThread>
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
    do { term_pump(); QApplication::processEvents(); if (predicate()) return true; QThread::msleep(10); } while (timer.elapsed() < 5000);
    return false;
}
int path_row(QListWidget *popup, const QString &command, int kind = 3) {
    for (int row = 0; row < popup->count(); ++row) {
        const auto *item = popup->item(row);
        if (item->data(Qt::UserRole + 1).toInt() == kind && item->data(Qt::UserRole).toString() == command) return row;
    }
    return -1;
}
}

bool selftest_ui::verify_environment(MainWindow &window, SendBar *bar, uint32_t tab, const QString &directory, const QString &png) {
    QFile fixture(QDir(directory).filePath(QStringLiteral("ENV_ONLY.txt")));
    if (!fixture.open(QIODevice::WriteOnly) || fixture.write("B3C3_PATH_ONCE\n") != 15) return false;
    fixture.close();
    QFile unsafe(QDir(directory).filePath(QStringLiteral("ENV unsafe.txt")));
    if (!unsafe.open(QIODevice::WriteOnly)) return false;
    unsafe.close();
    if (!QDir(directory).mkdir(QStringLiteral("ENV_DIR"))) return false;
    auto *input = bar->input();
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendEnvironmentPaths"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup || !activate_window(window)) return false;
#ifdef Q_OS_WIN
    const QString prefix = QStringLiteral("Get-Content ");
#else
    const QString prefix = QStringLiteral("cat ");
#endif
    const QString query = prefix + QStringLiteral("./ENV_");
    const QString command = prefix + QStringLiteral("./ENV_ONLY.txt");
#ifdef Q_OS_WIN
    // Exercise output arriving before the next real prompt/history hook. Keep
    // the normal isolated-shell integration; only its next return is delayed.
    const QByteArray prompt_fixture(
        "$global:ttB01Prompt=$function:prompt; $global:ttB01Delay=1; "
        "function global:prompt { if ($global:ttB01Delay -eq 0) { Start-Sleep -Milliseconds 150 }; "
        "$global:ttB01Delay--; & $global:ttB01Prompt }; Write-Output ('B01_'+'PROMPT_READY')\r");
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(prompt_fixture.data()), size_t(prompt_fixture.size())));
    if (!wait_for([&] {
        const auto screen = q(term_screen_text(tab)).trimmed();
        return screen.contains(QStringLiteral("B01_PROMPT_READY")) && screen.endsWith(QLatin1Char('>'));
    })) return false;
#endif
    const auto sent = bar->sent_count();
    input->setText(query); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
    bool ok = !button->isChecked() && path_row(popup, command) < 0;
    button->click();
    if (!wait_for([&] { return popup->isVisible() && path_row(popup, command) >= 0; })) return false;
    ok &= button->isChecked() && bar->sent_count() == sent;
    if (!png.isEmpty()) {
        ok &= window.grab().save(png + QStringLiteral(".environment.png"));
        ok &= popup->grab().save(png + QStringLiteral(".environment-popup.png"));
    }
    key(input, Qt::Key_Escape);
    ok &= !popup->isVisible() && input->text() == query && bar->sent_count() == sent;
    key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!wait_for([&] { return path_row(popup, command) >= 0; })) return false;
    popup->setCurrentRow(path_row(popup, command)); key(input, Qt::Key_Tab);
    ok &= input->text() == command && !popup->isVisible() && bar->sent_count() == sent;
    QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true, 1);
    QApplication::sendEvent(input, &held);
    ok &= bar->sent_count() == sent;
    key(input, Qt::Key_Return);
    // A rendered output line is not the shell's next completed prompt. Wait for
    // its real command boundary before snapshotting another completion context.
    ok &= wait_for([&] {
        const auto screen = q(term_screen_text(tab)).trimmed();
        if (!screen.contains(QStringLiteral("B3C3_PATH_ONCE")) ||
            !(screen.endsWith(QLatin1Char('>')) || screen.endsWith(QLatin1Char('$')) || screen.endsWith(QLatin1Char('#')))) return false;
        for (const auto &block : term_commands(tab)) if (q(block.title) == command && block.finished) return true;
        return false;
    });
    ok &= bar->sent_count() == sent + 1 && q(term_screen_text(tab)).count(QStringLiteral("B3C3_PATH_ONCE")) == 1;
    const auto output_context = completion_context(tab);
    const QString output_screen = q(term_screen_text(tab)).trimmed();
    bool output_finished = false;
    for (const auto &block : term_commands(tab)) output_finished |= q(block.title) == command && block.finished;
    std::printf("[environment] after-output prompt=%d finished=%d\n", output_screen.endsWith(QLatin1Char('>')) ||
        output_screen.endsWith(QLatin1Char('$')) || output_screen.endsWith(QLatin1Char('#')), output_finished);
    std::printf("[environment] opt-in/async/cancel/draft/explicit shell send once: %d\n", ok);
    input->setText(query); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!wait_for([&] { return path_row(popup, prefix + QStringLiteral("./ENV_DIR/")) >= 0; })) return false;
    term_set_reading(tab, true); key(input, Qt::Key_Return);
    const bool stale_rejected = input->text() == query && bar->sent_count() == sent + 1;
    ok &= stale_rejected;
    term_set_reading(tab, false);
    input->setText(prefix + QStringLiteral("./missing-directory/"));
    const auto error_context = completion_context(tab);
    key(input, Qt::Key_Space, Qt::ControlModifier);
    const bool io_feedback = wait_for([&] {
        for (int row = 0; row < popup->count(); ++row) if (popup->item(row)->text()
            == i18n::tr("无法读取路径，请检查目录和访问权限后重试。")) return popup->isVisible();
        return false;
    });
    const auto error_after = completion_context(tab);
    std::printf("[environment] stale=%d IO=%d history-changed=%d directory-changed=%d output-history-changed=%d focus=%d popup=%d\n",
        stale_rejected, io_feedback, error_context.history_revision != error_after.history_revision,
        error_context.directory_revision != error_after.directory_revision,
        output_context.history_revision != error_after.history_revision, input->hasFocus(), popup->isVisible());
    ok &= io_feedback;
    key(input, Qt::Key_Escape); button->click();
    ok &= !button->isChecked() && bar->sent_count() == sent + 1;
    key(input, Qt::Key_Escape);
    std::printf("[environment] stale reading rejected/IO feedback/disable retains draft: %d\n", ok);
    QFile quoted(QDir(directory).filePath(QStringLiteral("QUOTE space's.txt")));
    const QByteArray payload("B3C_QUOTED_ONCE\n");
    if (!quoted.open(QIODevice::WriteOnly) || quoted.write(payload) != payload.size()) return false;
    quoted.close();
    auto *syntax = bar->findChild<QComboBox *>(QStringLiteral("sendPathSyntax"));
    if (!syntax) return false;
    ok &= syntax->currentIndex() == 0 && !syntax->isEnabled();
#ifdef Q_OS_WIN
    const QString quote_prefix = QStringLiteral("Get-Content -LiteralPath ");
    const QString quote_command = quote_prefix + QStringLiteral("'./QUOTE space''s.txt'");
    const int dialect = 2;
#else
    const QString quote_prefix = QStringLiteral("cat ");
    const QString quote_command = quote_prefix + QStringLiteral("'./QUOTE space'\\''s.txt'");
    const int dialect = 1;
#endif
    const QString quote_query = quote_prefix + QStringLiteral("'./QUOTE");
    input->setText(quote_query); input->setFocus(); button->click();
    syntax->setCurrentIndex(dialect);
    if (!wait_for([&] { return popup->isVisible() && path_row(popup, quote_command) >= 0; })) return false;
    if (!png.isEmpty()) {
        ok &= window.grab().save(png + QStringLiteral(".quoted-path.png"));
        ok &= popup->grab().save(png + QStringLiteral(".quoted-path-popup.png"));
    }
    syntax->setCurrentIndex(0); key(input, Qt::Key_Return);
    ok &= input->text() == quote_query && bar->sent_count() == sent + 1;
    syntax->setCurrentIndex(dialect);
    if (!wait_for([&] { return popup->isVisible() && path_row(popup, quote_command) >= 0; })) return false;
    popup->setCurrentRow(path_row(popup, quote_command)); key(input, Qt::Key_Tab);
    ok &= input->text() == quote_command && bar->sent_count() == sent + 1;
    QApplication::sendEvent(input, &held);
    ok &= bar->sent_count() == sent + 1;
    key(input, Qt::Key_Return);
    ok &= wait_for([&] { return q(term_screen_text(tab)).count(QStringLiteral("B3C_QUOTED_ONCE")) == 1; });
    ok &= bar->sent_count() == sent + 2;
    button->click(); key(input, Qt::Key_Escape);
    ok &= !syntax->isEnabled();
    std::printf("[environment-quote] explicit syntax/stale mode/draft/fresh literal send: %d\n", ok);
    return ok;
}

bool selftest_ui::verify_remote_environment(const QString &png) {
    bool valid_port = false;
    if (qEnvironmentVariableIsEmpty("TIDETERM_TEST_SFTP_PORT")) return true;
    const uint port = qEnvironmentVariable("TIDETERM_TEST_SFTP_PORT").toUInt(&valid_port);
    if (!valid_port || !port || port > 65535) return false;
    auto cfg = sess_form_get("", 1);
    cfg.ssh_host = "127.0.0.1"; cfg.ssh_port = port;
    cfg.ssh_user = "fixture"; cfg.ssh_password = "fixture-password"; cfg.ssh_auth = 0;
    cfg.sftp_enabled = true; cfg.sysmon_enabled = false;
    const auto tab = sess_open_draft(cfg);
    if (tab == UINT32_MAX) return false;
    MainWindow window(false); window.add_tab_with(tab); window.show();
    if (!activate_window(window) || !wait_for([&] {
        const auto context = completion_query(tab, "").context;
        return context.ready && q(context.cwd) == QStringLiteral("/env-a");
    })) return false;
    auto *bar = window.page_sendbar(window.current_page());
    if (!bar) return false;
    auto *input = bar->input();
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendEnvironmentPaths"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup) return false;
    const QString query = QStringLiteral("cat ./ENV_");
    const QString command = QStringLiteral("cat ./ENV_ONLY.txt");
    auto remote_row = [&] {
        return path_row(popup, command, 4);
    };
    input->setText(query); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
    bool ok = !button->isChecked() && remote_row() < 0;
    button->click();
    if (!wait_for([&] { return popup->isVisible() && remote_row() >= 0; })) return false;
    ok &= popup->item(remote_row())->text().startsWith(i18n::tr("SSH 路径"));
    ok &= bar->sent_count() == 0 && !q(term_screen_text(tab)).contains(QStringLiteral("FIXTURE_RECEIVED"));
    if (!png.isEmpty()) {
        ok &= window.grab().save(png + QStringLiteral(".ssh-path.png"));
        ok &= popup->grab().save(png + QStringLiteral(".ssh-path-popup.png"));
    }
    popup->setCurrentRow(remote_row()); key(input, Qt::Key_Tab);
    ok &= input->text() == command && !popup->isVisible() && bar->sent_count() == 0;
    QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true, 1);
    QApplication::sendEvent(input, &held);
    ok &= bar->sent_count() == 0;
    key(input, Qt::Key_Return);
    ok &= wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("FIXTURE_RECEIVED")); });
    ok &= bar->sent_count() == 1;
    input->setText(query); input->setFocus(); key(input, Qt::Key_Space, Qt::ControlModifier);
    if (!wait_for([&] { return remote_row() >= 0; })) return false;
    term_set_reading(tab, true); key(input, Qt::Key_Return);
    ok &= input->text() == query && bar->sent_count() == 1;
    term_set_reading(tab, false);
    auto *syntax = bar->findChild<QComboBox *>(QStringLiteral("sendPathSyntax"));
    if (!syntax) return false;
    ok &= syntax->currentIndex() == 0;
    input->setText(QStringLiteral("cat './QUOTE")); input->setFocus();
    syntax->setCurrentIndex(1);
    const QString quoted = QStringLiteral("cat './QUOTE space'\\''s.txt'");
    if (!wait_for([&] { return popup->isVisible() && path_row(popup, quoted, 4) >= 0; })) return false;
    popup->setCurrentRow(path_row(popup, quoted, 4)); key(input, Qt::Key_Tab);
    ok &= input->text() == quoted && bar->sent_count() == 1;
    button->click(); key(input, Qt::Key_Escape);
    ok &= !button->isChecked() && bar->sent_count() == 1;
    ok &= verify_ssh_programs(window, bar, tab, png);
    term_close_tab(tab);
    std::printf("[environment-ssh] SFTP source/opt-in/draft/fresh send/reading: %d\n", ok);
    return ok;
}
