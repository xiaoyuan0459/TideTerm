#include "cpp/chrome/selftest_shell_report.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &f) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); QApplication::processEvents(); if (f()) return true; QThread::msleep(10); } while (timer.elapsed() < 12000);
    return false;
}
void key(QWidget *widget, int code, Qt::KeyboardModifiers mods = Qt::NoModifier) {
    QKeyEvent event(QEvent::KeyPress, code, mods); QApplication::sendEvent(widget, &event);
}
}
bool selftest_ui::verify_docker(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
    const QString root = qEnvironmentVariable("TIDETERM_TEST_DOCKER_ROOT");
    const QString exe = qEnvironmentVariable("TIDETERM_TEST_DOCKER_EXE");
    if (root.isEmpty() || exe.isEmpty()) {
        std::printf("[docker] not run: requires isolated Docker CLI/context and loopback Engine fixture\n"); return true;
    }
    const QString canonical = QFileInfo(root).canonicalFilePath();
    const QString temp = QFileInfo(QDir::tempPath()).canonicalFilePath() + QLatin1Char('/');
    if (!canonical.startsWith(temp) || !QFileInfo(exe).isFile()) return false;
    auto stats = [&] {
        QFile file(QDir(root).filePath(QStringLiteral("stats.json")));
        if (!file.open(QIODevice::ReadOnly)) return QJsonObject{};
        return QJsonDocument::fromJson(file.readAll()).object();
    };
    const auto before = stats(); if (before.isEmpty()) return false;
    const QString config = QDir(root).filePath(QStringLiteral("config")).replace(QLatin1Char('\''), QStringLiteral("''"));
    const QString path = QFileInfo(exe).absolutePath().replace(QLatin1Char('\''), QStringLiteral("''"));
#ifdef Q_OS_WIN
    const uint8_t dialect = 2;
    const auto setup = (QStringLiteral("& { foreach ($n in [Environment]::GetEnvironmentVariables().Keys) { if ([string]$n -like 'DOCKER_*') { [Environment]::SetEnvironmentVariable($n,$null,'Process') } }; $env:DOCKER_CONFIG='")
        + config + QStringLiteral("'; $env:PATH='") + path + QStringLiteral(";'+$env:PATH; $env:DOCKER_HOST='tcp://127.0.0.1:9'; $env:DOCKER_CONTEXT='wrong-context' }; Write-Output ('DOCKER_FIXTURE_'+'READY')\r")).toUtf8();
#else
    const uint8_t dialect = 1;
    const auto setup = (QStringLiteral("for n in ${!DOCKER_@}; do builtin unset \"$n\"; done; export DOCKER_CONFIG='") + config
        + QStringLiteral("'; export PATH='") + path + QStringLiteral(":'\"$PATH\"; builtin printf 'DOCKER_FIXTURE_%s\\n' READY\r")).toUtf8();
#endif
    const auto revision = completion_context(tab).history_revision;
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(setup.constData()), size_t(setup.size())));
    if (!wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("DOCKER_FIXTURE_READY"))
        && completion_context(tab).history_revision != revision
        && (dialect == 1 || q(term_screen_text(tab)).trimmed().endsWith(QLatin1Char('>'))); })) return false;
    auto *input = bar->input(); const auto original = input->text(); const auto sent = bar->sent_count();
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup) return false;
    bool ok = true, ready = false, failure = false;
    QTimer watchdog; watchdog.setSingleShot(true);
    QObject::connect(&watchdog, &QTimer::timeout, &window, [&] { ok = false; if (auto *d = dynamic_cast<QDialog *>(QApplication::activeModalWidget())) d->reject(); });
    for (bool collect : {false, true}) {
        watchdog.start(35000);
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog) { ok = false; return; }
            auto *mode = dialog->findChild<QComboBox *>(QStringLiteral("shellReportMode"));
            auto *syntax = dialog->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
            auto *name = dialog->findChild<QLineEdit *>(QStringLiteral("shellLookupName"));
            auto *refresh = dialog->findChild<QPushButton *>(QStringLiteral("shellReportRefresh"));
            auto *confirm = dialog->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
            auto *send = dialog->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
            auto *preview = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPreview"));
            auto *result = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
            if (!mode || !syntax || !name || !refresh || !confirm || !send || !preview || !result) { ok = false; dialog->reject(); return; }
            mode->setCurrentIndex(3); syntax->setCurrentIndex(dialect);
            name->setText(QStringLiteral("bad name")); refresh->click();
            ok &= shell_report_poll(tab).state == 12 && !send->isEnabled();
            name->setText(QStringLiteral("fixture")); refresh->click();
            ok &= shell_report_poll(tab).state == 1 && !send->isEnabled() && stats() == before
                && preview->toPlainText() == q(shell_report_poll(tab).command) && input->text() == original;
            if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".docker-preview.png"));
            if (collect) {
                auto query = [&](uint8_t state, bool backend_only = false) {
                    const auto history = completion_context(tab).history_revision;
                    refresh->click(); confirm->setChecked(true); send->click();
                    auto received_report = [&] { return shell_report_poll(tab).state == state && completion_context(tab).history_revision != history
                        && (dialect == 1 || q(term_screen_text(tab)).trimmed().endsWith(QLatin1Char('>'))); };
                    bool received = false;
                    if (backend_only) {
                        // Keep the real terminal reader/prompt hook running while
                        // withholding Qt's 200 ms view refresh. Closing must use
                        // the received backend state, not the old waiting view.
                        QElapsedTimer clock; clock.start();
                        do { term_pump(); if (received_report()) { received = true; break; } QThread::msleep(10); } while (clock.elapsed() < 12000);
                    } else received = wait_for(received_report);
                    std::printf("[docker] query expected=%u actual=%u history-finished=%d ready=%d\n", unsigned(state), unsigned(shell_report_poll(tab).state), completion_context(tab).history_revision != history, received);
                    return received;
                };
                ready = query(3) && wait_for([&] { return result->toPlainText().contains(QStringLiteral("ui_alpha")); });
                const auto first = shell_report_poll(tab).token;
                name->setText(QStringLiteral("missing-fixture"));
                ok &= shell_report_poll(tab).state == 0 && completion_docker_draft(completion_context(tab), first, "docker logs ui_", "bad-id").empty();
                failure = query(11);
                name->setText(QStringLiteral("fixture")); ready &= query(3, true);
                ok &= ready && failure && bar->sent_count() == sent;
                std::printf("[docker] close-race backend=%u view-result-empty=%d\n", unsigned(shell_report_poll(tab).state), result->toPlainText().isEmpty());
            }
            dialog->reject();
        });
        button->click(); watchdog.stop();
        ok &= shell_report_poll(tab).state == (collect ? 3 : 0);
    }
    std::printf("[docker] after-close backend=%u\n", unsigned(shell_report_poll(tab).state));
    // Reopen only to inspect the preserved snapshot; this must issue no query.
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog) { ok = false; return; }
        auto *result = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
        ok &= result && result->toPlainText().contains(QStringLiteral("ui_alpha"));
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".docker-ready.png"));
        dialog->reject();
    });
    button->click();
    auto show = [&] {
        if (!activate_window(window)) return -1;
        input->setFocus(); input->setText(QStringLiteral("docker logs ui_")); key(input, Qt::Key_Space, Qt::ControlModifier);
        for (int i = 0; i < popup->count(); ++i) if (popup->item(i)->data(Qt::UserRole + 1).toInt() == 10) return i;
        return -1;
    };
    int row = show(); if (row < 0) { std::printf("[docker] no candidate state=%u focus=%d popup=%d\n", unsigned(shell_report_poll(tab).state), input->hasFocus(), popup->isVisible()); return false; }
    ok &= popup->isVisible() && popup->item(row)->toolTip().contains(QStringLiteral("context: fixture"));
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".docker-suggestions.png"));
    popup->setCurrentRow(row); key(input, Qt::Key_Tab);
    const bool draft = input->text() == QStringLiteral("docker --context 'fixture' container logs '") + QString(63, QLatin1Char('0')) + QStringLiteral("1'");
    QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true); QApplication::sendEvent(input, &held);
    const bool silent = bar->sent_count() == sent;
    row = show(); if (row < 0) { std::printf("[docker] reusable candidate unavailable state=%u\n", unsigned(shell_report_poll(tab).state)); return false; }
    popup->setCurrentRow(row); shell_report_clear(tab); key(input, Qt::Key_Return);
    const bool stale = !popup->isVisible() && input->text() == QStringLiteral("docker logs ui_") && bar->sent_count() == sent;
    const auto after = stats();
    const bool api = after.value(QStringLiteral("lists")).toInt() == before.value(QStringLiteral("lists")).toInt() + 2
        && after.value(QStringLiteral("forbidden")).toInt() == 0 && after.value(QStringLiteral("bad_query")).toInt() == 0;
    input->clear(); ok &= draft && silent && stale && api;
    std::printf("[docker] ready=%d failure=%d draft=%d held/silent=%d stale=%d bounded-list-only=%d all=%d\n", ready, failure, draft, silent, stale, api, ok);
    return ok;
}
