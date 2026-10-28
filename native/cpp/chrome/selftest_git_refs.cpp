#include "cpp/chrome/selftest_shell_report.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QElapsedTimer>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QProcess>
#include <QProcessEnvironment>
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

bool selftest_ui::verify_git_refs(MainWindow &window, SendBar *bar, uint32_t tab, const QString &directory, const QString &png) {
    auto git = [&](const QStringList &args) {
        auto env = QProcessEnvironment::systemEnvironment();
        for (const auto &name : env.keys()) if (name.startsWith(QStringLiteral("GIT_"))) env.remove(name);
#ifdef Q_OS_WIN
        env.insert(QStringLiteral("GIT_CONFIG_GLOBAL"), QStringLiteral("NUL"));
#else
        env.insert(QStringLiteral("GIT_CONFIG_GLOBAL"), QStringLiteral("/dev/null"));
#endif
        env.insert(QStringLiteral("GIT_CONFIG_NOSYSTEM"), QStringLiteral("1"));
        QProcess process; process.setProcessEnvironment(env); process.setWorkingDirectory(directory);
        process.start(QStringLiteral("git"), QStringList{QStringLiteral("-c"), QStringLiteral("core.hooksPath="), QStringLiteral("-c"), QStringLiteral("init.templateDir=")} + args);
        return process.waitForStarted(5000) && process.waitForFinished(10000) && process.exitCode() == 0;
    };
    if (!git({QStringLiteral("init"), QStringLiteral("-q")})
        || !git({QStringLiteral("-c"), QStringLiteral("user.name=Fixture"), QStringLiteral("-c"), QStringLiteral("user.email=fixture@invalid"), QStringLiteral("commit"), QStringLiteral("-qm"), QStringLiteral("fixture"), QStringLiteral("--allow-empty")})
        || !git({QStringLiteral("branch"), QStringLiteral("ui-main")}) || !git({QStringLiteral("tag"), QStringLiteral("ui-v1")})) return false;
    auto *input = bar->input(); const auto original = input->text(); const auto sent = bar->sent_count();
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup) return false;
#ifdef Q_OS_WIN
    const uint8_t dialect = 2;
    const QByteArray setup("& { foreach ($n in [Environment]::GetEnvironmentVariables().Keys) { if ([string]$n -like 'GIT_*') { [Environment]::SetEnvironmentVariable($n,$null,'Process') } }; $env:GIT_CONFIG_GLOBAL='NUL'; $env:GIT_CONFIG_NOSYSTEM='1' }; Write-Output ('GIT_FIXTURE_'+'READY')\r");
#else
    const uint8_t dialect = 1;
    const QByteArray setup("for n in ${!GIT_@}; do builtin unset \"$n\"; done; export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1; builtin printf 'GIT_FIXTURE_%s\\n' READY\r");
#endif
    const auto setup_revision = completion_context(tab).history_revision;
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(setup.constData()), size_t(setup.size())));
    if (!wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("GIT_FIXTURE_READY"))
        && completion_context(tab).history_revision != setup_revision
        && (dialect == 1 || q(term_screen_text(tab)).trimmed().endsWith(QLatin1Char('>'))); })) return false;
    bool ok = true, previewed = false, ready = false;
    QTimer watchdog; watchdog.setSingleShot(true);
    QObject::connect(&watchdog, &QTimer::timeout, &window, [&] { ok = false; if (auto *d = dynamic_cast<QDialog *>(QApplication::activeModalWidget())) d->reject(); });
    for (bool send_report : {false, true}) {
        watchdog.start(20000);
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog) { ok = false; return; }
            auto *mode = dialog->findChild<QComboBox *>(QStringLiteral("shellReportMode"));
            auto *syntax = dialog->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
            auto *confirm = dialog->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
            auto *send = dialog->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
            auto *preview = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPreview"));
            auto *path = dialog->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
            if (!mode || !syntax || !confirm || !send || !preview || !path) { ok = false; dialog->reject(); return; }
            mode->setCurrentIndex(2); syntax->setCurrentIndex(dialect);
            const auto report = shell_report_poll(tab);
            previewed = report.state == 1 && report.git && !send->isEnabled()
                && preview->toPlainText() == q(report.command) && q(report.command).contains(QStringLiteral("for-each-ref"));
            ok &= previewed && input->text() == original && bar->sent_count() == sent;
            if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".git-preview.png"));
            if (send_report) {
                const auto revision = completion_context(tab).history_revision;
                confirm->setChecked(true); send->click();
                ready = wait_for([&] { return shell_report_poll(tab).state == 3 && path->toPlainText().contains(QStringLiteral("refs/tags/ui-v1"))
                    && completion_context(tab).history_revision != revision && (dialect == 1 || q(term_screen_text(tab)).trimmed().endsWith(QLatin1Char('>'))); });
                ok &= ready && !send->isEnabled() && bar->sent_count() == sent;
                if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".git-ready.png"));
            }
            dialog->reject();
        });
        button->click(); watchdog.stop();
        ok &= shell_report_poll(tab).state == (send_report ? 3 : 0);
    }
    auto show = [&] {
        if (!activate_window(window)) return -1;
        input->setFocus(); input->setText(QStringLiteral("git show ui-")); key(input, Qt::Key_Space, Qt::ControlModifier);
        for (int i = 0; i < popup->count(); ++i) if (popup->item(i)->data(Qt::UserRole + 1).toInt() == 9) return i;
        return -1;
    };
    int row = show(); if (row < 0) return false;
    ok &= popup->isVisible() && popup->item(row)->text().contains(QStringLiteral("Git"))
        && popup->item(row)->toolTip().contains(QStringLiteral("refs/tags/ui-v1"));
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".git-suggestions.png"));
    popup->setCurrentRow(row); key(input, Qt::Key_Tab);
    const bool draft = input->text() == QStringLiteral("git show 'refs/tags/ui-v1'") && !popup->isVisible() && shell_report_poll(tab).state == 3;
    QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true); QApplication::sendEvent(input, &held);
    const bool silent = bar->sent_count() == sent;
    row = show(); if (row < 0) return false;
    popup->setCurrentRow(row); shell_report_clear(tab); key(input, Qt::Key_Return);
    const bool stale = !popup->isVisible() && input->text() == QStringLiteral("git show ui-") && bar->sent_count() == sent;
    input->clear(); ok &= draft && silent && stale;
    std::printf("[git-refs] preview=%d ready=%d draft=%d held/silent=%d stale=%d all=%d\n", previewed, ready, draft, silent, stale, ok);
    return ok;
}
