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
#include <functional>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &f, bool events = true) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); if (events) QApplication::processEvents(); if (f()) return true; QThread::msleep(10); } while (timer.elapsed() < 12000);
    return false;
}
void key(QWidget *widget, int code, Qt::KeyboardModifiers mods = Qt::NoModifier) {
    QKeyEvent event(QEvent::KeyPress, code, mods); QApplication::sendEvent(widget, &event);
}
}
static bool verify_kube_resource(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png, uint8_t resource) {
    const QString kind = resource == 0 ? QStringLiteral("pod") : resource == 1 ? QStringLiteral("service") : QStringLiteral("deployment");
    const QString query_text = QStringLiteral("kubectl get ") + kind + QLatin1Char(' ') + kind;
    const QString identity = QStringLiteral("fixture-") + kind + QStringLiteral("-uid-000");
    const QString entry_name = kind + QStringLiteral("-000");
    const QString root = qEnvironmentVariable("TIDETERM_TEST_KUBE_ROOT");
    const QString exe = qEnvironmentVariable("TIDETERM_TEST_KUBE_EXE");
    if (root.isEmpty() || exe.isEmpty()) {
        std::printf("[kube] not run: requires isolated kubectl/TLS API/exec credential fixture\n"); return true;
    }
    if (!wait_for([&] { const auto state = terminal_completion_query(tab); return state.batch.context.ready && state.query.empty(); })) {
        std::printf("[kube] resource=%u prior shell prompt did not settle\n", unsigned(resource)); return false;
    }
    const QString canonical = QFileInfo(root).canonicalFilePath();
    const QString temp = QFileInfo(QDir::tempPath()).canonicalFilePath() + QLatin1Char('/');
    if (!canonical.startsWith(temp) || !QFileInfo(exe).isFile()) return false;
    auto read = [&](const QString &name) { QFile f(QDir(root).filePath(name)); return f.open(QIODevice::ReadOnly) ? f.readAll() : QByteArray{}; };
    auto stats = [&] { return QJsonDocument::fromJson(read(QStringLiteral("stats.json"))).object(); };
    const auto before = stats(); const int plugins = read(QStringLiteral("plugin-count.txt")).toInt();
    if (before.isEmpty()) return false;
    const QString config = QDir(root).filePath(QStringLiteral("scope space's.json"));
    const auto escape = [](QString s) { return s.replace(QLatin1Char('\''), QStringLiteral("''")); };
    const auto setup = (QStringLiteral("$env:HOME='") + escape(root) + QStringLiteral("'; $env:USERPROFILE=$env:HOME; $env:KUBECONFIG='wrong-config'; $env:PATH='")
        + escape(QFileInfo(exe).absolutePath()) + QStringLiteral(";'+$env:PATH; Write-Output ('KUBE_FIXTURE_'+'READY')\r")).toUtf8();
    const auto revision = completion_context(tab).history_revision;
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(setup.constData()), size_t(setup.size())));
    if (!wait_for([&] { const auto state = terminal_completion_query(tab);
        return q(term_screen_text(tab)).contains(QStringLiteral("KUBE_FIXTURE_READY"))
            && completion_context(tab).history_revision != revision && state.batch.context.ready && state.query.empty(); })) {
        const auto context = completion_context(tab);
        const auto screen = term_screen_text(tab);
        std::printf("[kube] resource=%u setup ready=%d history=%llu/%llu screen=[%.*s]\n", unsigned(resource), context.ready,
            static_cast<unsigned long long>(context.history_revision), static_cast<unsigned long long>(revision), int(screen.size()), screen.data());
        return false;
    }
    auto *input = bar->input(); const auto original = input->text(); const auto sent = bar->sent_count();
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup) return false;
    bool ok = true, ready = false, failure = false, gate = false;
    QTimer watchdog; watchdog.setSingleShot(true);
    QObject::connect(&watchdog, &QTimer::timeout, &window, [&] { ok = false; if (auto *d = dynamic_cast<QDialog *>(QApplication::activeModalWidget())) d->reject(); });
    for (bool collect : {false, true}) {
        watchdog.start(35000);
        QTimer::singleShot(0, &window, [&] {
            auto *d = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!d) { ok = false; return; }
            auto *mode = d->findChild<QComboBox *>(QStringLiteral("shellReportMode"));
            auto *syntax = d->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
            auto *context = d->findChild<QLineEdit *>(QStringLiteral("shellLookupName"));
            auto *file = d->findChild<QLineEdit *>(QStringLiteral("kubeConfigPath"));
            auto *ns = d->findChild<QLineEdit *>(QStringLiteral("kubeNamespace"));
            auto *resource_box = d->findChild<QComboBox *>(QStringLiteral("kubeResource"));
            auto *refresh = d->findChild<QPushButton *>(QStringLiteral("shellReportRefresh"));
            auto *confirm = d->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
            auto *credentials = d->findChild<QCheckBox *>(QStringLiteral("kubeCredentialsConfirm"));
            auto *send = d->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
            auto *result = d->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
            if (!mode || !syntax || !context || !file || !ns || !resource_box || !refresh || !confirm || !credentials || !send || !result) { ok = false; d->reject(); return; }
            mode->setCurrentIndex(4); syntax->setCurrentIndex(2); file->setText(config); context->setText(QStringLiteral("fixture"));
            resource_box->setCurrentIndex(resource);
            ns->setText(QStringLiteral("-bad")); refresh->click(); ok &= shell_report_poll(tab).state == 13 && !send->isEnabled();
            ns->setText(QStringLiteral("fixture")); refresh->click();
            const auto preview = shell_report_poll(tab);
            confirm->setChecked(true);
            gate = preview.state == 1 && !send->isEnabled() && shell_report_send(tab, preview.token) == 15
                && shell_kube_send(tab, preview.token, false) == 15 && stats() == before
                && read(QStringLiteral("plugin-count.txt")).toInt() == plugins && input->text() == original;
            credentials->setChecked(true); gate &= send->isEnabled(); ok &= gate;
            if (!png.isEmpty()) ok &= d->grab().save(png + QStringLiteral(".kube-preview.png"));
            if (collect) {
                auto query = [&](uint8_t state, bool events = true) {
                    const auto history = completion_context(tab).history_revision;
                    refresh->click(); confirm->setChecked(true); credentials->setChecked(true); send->click();
                    const bool received = wait_for([&] { return shell_report_poll(tab).state == state && completion_context(tab).history_revision != history
                        && q(term_screen_text(tab)).trimmed().endsWith(QLatin1Char('>')); }, events);
                    std::printf("[kube] query expected=%u actual=%u ready=%d\n", unsigned(state), unsigned(shell_report_poll(tab).state), received);
                    return received;
                };
                ready = query(3) && wait_for([&] { return result->toPlainText().contains(entry_name); });
                const auto first = shell_report_poll(tab).token;
                context->setText(QStringLiteral("missing-fixture"));
                const auto draft_query = query_text.toUtf8(); const auto uid = identity.toUtf8();
                ok &= shell_report_poll(tab).state == 0 && completion_kube_draft(completion_context(tab), first,
                    rust::Str(draft_query.data(),size_t(draft_query.size())),rust::Str(uid.data(),size_t(uid.size()))).empty();
                failure = query(14); context->setText(QStringLiteral("fixture")); ready &= query(3, false);
                ok &= ready && failure && bar->sent_count() == sent;
            }
            d->reject();
        });
        button->click(); watchdog.stop(); ok &= shell_report_poll(tab).state == (collect ? 3 : 0);
    }
    QTimer::singleShot(0, &window, [&] {
        auto *d = dynamic_cast<QDialog *>(QApplication::activeModalWidget()); if (!d) { ok = false; return; }
        auto *result = d->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
        ok &= result && result->toPlainText().contains(entry_name);
        if (!png.isEmpty()) ok &= d->grab().save(png + QStringLiteral(".kube-ready.png")); d->reject();
    }); button->click();
    auto show = [&] {
        if (!selftest_ui::activate_window(window)) return -1;
        input->setFocus(); input->setText(query_text); key(input, Qt::Key_Space, Qt::ControlModifier);
        for (int i = 0; i < popup->count(); ++i) if (popup->item(i)->data(Qt::UserRole + 1).toInt() == 11) return i;
        return -1;
    };
    int row = show(); if (row < 0) return false;
    ok &= popup->isVisible() && popup->item(row)->toolTip().contains(identity);
    if (!png.isEmpty()) ok &= popup->grab().save(png + QStringLiteral(".kube-suggestions.png"));
    popup->setCurrentRow(row); key(input, Qt::Key_Tab);
    const bool draft = input->text() == QStringLiteral("kubectl '--kubeconfig=") + escape(config) + QStringLiteral("' '--context=fixture' '--namespace=fixture' get ") + kind + QStringLiteral(" '") + entry_name + QLatin1Char('\'');
    QKeyEvent held(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true); QApplication::sendEvent(input, &held);
    const bool silent = bar->sent_count() == sent;
    row = show(); if (row < 0) return false;
    popup->setCurrentRow(row); shell_report_clear(tab); key(input, Qt::Key_Return);
    const bool stale = !popup->isVisible() && input->text() == query_text && bar->sent_count() == sent;
    const auto after = stats();
    const bool api = after.value(QStringLiteral("lists")).toInt() == before.value(QStringLiteral("lists")).toInt() + 2
        && read(QStringLiteral("plugin-count.txt")).toInt() == plugins + 2
        && after.value(QStringLiteral("forbidden")).toInt() == 0 && after.value(QStringLiteral("bad_scope")).toInt() == 0 && after.value(QStringLiteral("bad_auth")).toInt() == 0;
    input->clear(); ok &= draft && silent && stale && api;
    std::printf("[kube] resource=%u gate=%d ready=%d failure=%d draft=%d silent=%d stale=%d scoped-list-only=%d all=%d\n", unsigned(resource),gate, ready, failure, draft, silent, stale, api, ok);
    return ok;
}
bool selftest_ui::verify_kube(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
    for (uint8_t resource=0;resource<3;++resource) if (!verify_kube_resource(window,bar,tab,
        png.isEmpty() ? png : png+QStringLiteral(".resource-%1").arg(resource),resource)) return false;
    return true;
}
