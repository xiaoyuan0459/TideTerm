// Actual menu/button journeys and real local-shell delivery, without clipboard IO.
#include "cpp/chrome/selftest_senders.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QElapsedTimer>
#include <QLabel>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTemporaryDir>
#include <QTableWidget>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <cstdio>

namespace {
QString q(const rust::String &value) { return QString::fromUtf8(value.data(), int(value.size())); }
bool wait_for(const std::function<bool()> &ready, int timeout = 6000) {
    QElapsedTimer elapsed; elapsed.start();
    do {
        term_pump(); QApplication::processEvents();
        if (ready()) return true;
        QThread::msleep(10);
    } while (elapsed.elapsed() < timeout);
    return false;
}
QString command(const char *suffix) {
#ifdef Q_OS_WIN
    return QStringLiteral("Write-Output ('SENDER_'+'%1')").arg(QLatin1String(suffix));
#else
    return QStringLiteral("printf '%s%s\\n' SENDER_ %1").arg(QLatin1String(suffix));
#endif
}
SendBar *sender(QDialog *dialog) {
    return dialog ? dynamic_cast<SendBar *>(dialog->findChild<QWidget *>(QStringLiteral("sendBar"))) : nullptr;
}
bool has(uint32_t tab, const char *marker) { return q(term_screen_text(tab)).contains(QLatin1String(marker)); }
}

bool selftest_ui::verify_senders(const QString &png) {
    QTemporaryDir fixture;
    if (!fixture.isValid()) return false;
    MainWindow window(false), outside(false);
    auto cfg = sess_form_get("", 0);
    cfg.local_cwd = rust::String(fixture.path().toUtf8().toStdString());
#ifdef Q_OS_WIN
    cfg.local_shell = "pwsh";
#else
    cfg.local_shell = "bash";
#endif
    cfg.local_args = "";
    std::vector<uint32_t> ids;
    for (int i = 0; i < 3; ++i) {
        const auto id = sess_open_draft(cfg);
        if (id == UINT32_MAX) return false;
        ids.push_back(id); window.add_tab_with(id);
    }
    const auto external = sess_open_draft(cfg);
    if (external == UINT32_MAX) return false;
    outside.add_tab_with(external);
    window.resize(1320, 800); window.show();
    if (!activate_window(window) || !wait_for([&] {
        for (const auto id : ids) {
            const auto text = q(term_screen_text(id)).trimmed();
            if (!text.endsWith('>') && !text.endsWith('$') && !text.endsWith('#')) return false;
        }
        return true;
    }, 12000)) return false;
    auto *create = chrome::find_menu_action(window.menuBar(), QStringLiteral("tools:sender"));
    if (!create) return false;
    window.focus_session(ids[0]); create->trigger(); QApplication::processEvents();
    auto *first = window.findChildren<QDialog *>(QStringLiteral("senderWindow")).last();
    auto *a = sender(first);
    window.focus_session(ids[1]); create->trigger(); QApplication::processEvents();
    auto *second = window.findChildren<QDialog *>(QStringLiteral("senderWindow")).last();
    auto *b = sender(second);
    if (!a || !b || a == b) return false;
    window.focus_session(ids[0]);
    auto *sync_action = chrome::find_menu_action(window.menuBar(), QStringLiteral("tools:sync-input"));
    if (!sync_action) return false;
    auto configure_sync = [&](bool accept) {
        QTimer::singleShot(0, &window, [&, accept] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("syncChannels"));
            if (!dialog) return;
            dialog->findChild<QLineEdit *>(QStringLiteral("syncName0"))->setText(accept ? QStringLiteral("PAIR") : QStringLiteral("CANCELLED"));
            dialog->findChild<QLineEdit *>(QStringLiteral("syncName1"))->setText(QStringLiteral("SOLO"));
            auto *members = dialog->findChild<QTableWidget *>(QStringLiteral("syncMembers"));
            for (int row = 0; row < members->rowCount(); ++row) {
                const auto id = members->item(row, 0)->data(Qt::UserRole).toUInt();
                static_cast<QComboBox *>(members->cellWidget(row, 1))->setCurrentIndex(!accept ? 0 : id == ids[2] ? 2 : 1);
            }
            dialog->findChild<QDialogButtonBox *>()->button(accept ? QDialogButtonBox::Ok : QDialogButtonBox::Cancel)->click();
        });
        sync_action->trigger();
    };
    configure_sync(true); configure_sync(false); window.refresh_status();
    auto *sync_status = window.findChild<QLabel *>(QStringLiteral("statusSync"));
    const bool sync_indicator = sync_status && sync_status->text().contains(QStringLiteral("PAIR"))
        && term_sync_status(window.sync_owner(), ids[0]).members == 2;
    auto *origin = window.pane_view(0, 0);
    QKeyEvent typed(QEvent::KeyPress, 0, Qt::NoModifier, command("KEYS"));
    QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QStringLiteral("\r"));
    QApplication::sendEvent(origin, &typed); QApplication::sendEvent(origin, &enter);
    const bool sync_routed = wait_for([&] { return has(ids[0], "SENDER_KEYS") && has(ids[1], "SENDER_KEYS"); })
        && !has(ids[2], "SENDER_KEYS") && !has(external, "SENDER_KEYS");
    a->set_hex_mode(true);
    const QString a_text = QString::fromLatin1(command("PICKED").toUtf8().toHex());
    a->input()->setText(a_text); b->input()->setText(command("SECOND"));
    const bool independent = a->completion_source() == ids[0] && b->completion_source() == ids[1]
        && a->input()->text() == a_text && b->input()->text() == command("SECOND");
    auto choose = [&](bool accept) {
        QTimer::singleShot(0, &window, [&, accept] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("senderTargets"));
            if (!dialog) return;
            auto *list = dialog->findChild<QListWidget *>(QStringLiteral("senderTargetList"));
            for (int i = 0; i < list->count(); ++i)
                list->item(i)->setCheckState(accept && list->item(i)->data(Qt::UserRole).toUInt() == ids[2] ? Qt::Checked : Qt::Unchecked);
            auto *buttons = dialog->findChild<QDialogButtonBox *>();
            buttons->button(accept ? QDialogButtonBox::Ok : QDialogButtonBox::Cancel)->click();
        });
        a->findChild<QToolButton *>(QStringLiteral("sendPickTargets"))->click();
    };
    choose(true);
    auto *scope = a->findChild<QComboBox *>(QStringLiteral("sendTarget"));
    const QString picked = scope->currentText();
    choose(false);
    const bool cancelled = scope->currentIndex() == 3 && scope->currentText() == picked && a->sent_count() == 0;
    a->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    const bool selected = wait_for([&] { return has(ids[2], "SENDER_PICKED"); })
        && !has(ids[0], "SENDER_PICKED") && !has(ids[1], "SENDER_PICKED") && !has(external, "SENDER_PICKED");
    b->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    const bool own = wait_for([&] { return has(ids[1], "SENDER_SECOND"); })
        && !has(ids[0], "SENDER_SECOND") && !has(ids[2], "SENDER_SECOND") && a->input()->text() == a_text;
    a->set_hex_mode(false); a->input()->setText(command("READING"));
    term_set_reading(ids[2], true);
    const auto before = a->sent_count();
    a->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    const bool reading = a->sent_count() == before && !has(ids[2], "SENDER_READING")
        && !first->findChild<QLabel *>(QStringLiteral("senderStatus"))->text().isEmpty();
    term_set_reading(ids[2], false);
    scope->setCurrentIndex(2); a->input()->setText(command("WINDOW"));
    a->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    const bool bounded = wait_for([&] {
        return has(ids[0], "SENDER_WINDOW") && has(ids[1], "SENDER_WINDOW") && has(ids[2], "SENDER_WINDOW");
    }) && !has(external, "SENDER_WINDOW");
    scope->setCurrentIndex(0);
    a->findChild<QAction *>(QStringLiteral("sendCharacters"))->trigger();
    a->findChild<QDoubleSpinBox *>(QStringLiteral("sendCharacterInterval"))->setValue(0.01);
    a->input()->setText(command("FROZEN"));
    const auto frozen_before = a->sent_count();
    a->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    scope->setCurrentIndex(3); window.focus_session(ids[2]);
    const bool frozen = wait_for([&] { return a->sent_count() == frozen_before + 1 && has(ids[0], "SENDER_FROZEN"); })
        && !has(ids[2], "SENDER_FROZEN") && !has(ids[1], "SENDER_FROZEN");
    // Separate multiline editors retain only their own Sender's unfinished draft.
    auto multiline = [&](SendBar *bar, const QString &draft) {
        bar->findChild<QToolButton *>(QStringLiteral("sendMultiline"))->click(); QApplication::processEvents();
        auto *dialog = window.findChild<QDialog *>(QStringLiteral("multilineEditor"));
        if (!dialog) return QString();
        auto *text = dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"));
        const QString previous = text->toPlainText();
        if (!draft.isEmpty()) text->setPlainText(draft);
        dialog->reject(); QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        return previous;
    };
    multiline(a, QStringLiteral("UNSENT_A"));
    const bool draft_b = multiline(b, QStringLiteral("UNSENT_B")) != QStringLiteral("UNSENT_A");
    const bool drafts = draft_b && multiline(a, QString()) == QStringLiteral("UNSENT_A");
    a->findChild<QComboBox *>(QStringLiteral("sendNewline"))->setCurrentIndex(3);
    a->input()->setText(QStringLiteral("SHOULD_STOP_AFTER_FIRST"));
    const auto closed_before = a->sent_count();
    first->findChild<QLabel *>(QStringLiteral("senderStatus"))->clear();
    a->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    window.close_tab(2);
    const bool retired = wait_for([&] {
        return a->sent_count() == closed_before
            && first->findChild<QLabel *>(QStringLiteral("senderStatus"))->text() == i18n::tr("目标不可写、已关闭或重新连接，发送已停止");
    }, 1000);
    b->findChild<QAction *>(QStringLiteral("sendCharacters"))->trigger();
    b->findChild<QDoubleSpinBox *>(QStringLiteral("sendCharacterInterval"))->setValue(0.1);
    b->input()->setText(QStringLiteral("RECONNECT_STOPS_PENDING"));
    const auto reconnect_before = b->sent_count();
    second->findChild<QLabel *>(QStringLiteral("senderStatus"))->clear();
    b->findChild<QPushButton *>(QStringLiteral("sendStart"))->click();
    term_reconnect_tab(ids[1]);
    const bool reconnect = wait_for([&] {
        return b->sent_count() == reconnect_before
            && second->findChild<QLabel *>(QStringLiteral("senderStatus"))->text() == i18n::tr("目标不可写、已关闭或重新连接，发送已停止");
    }, 1200);
    if (!png.isEmpty()) first->grab().save(png + QStringLiteral(".sender-window.png"));
    first->close(); second->close(); QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QTimer::singleShot(0, &window, [&] {
        if (auto *dialog = window.findChild<QDialog *>(QStringLiteral("syncChannels")))
            dialog->findChild<QPushButton *>(QStringLiteral("syncStopAll"))->click();
    });
    sync_action->trigger(); window.refresh_status();
    const bool sync_stopped = term_sync_status(window.sync_owner(), ids[0]).channel == 0 && sync_status->text().isEmpty();
    std::printf("[senders] independent=%d picker-cancel=%d selected=%d own=%d reading=%d window-scope=%d frozen=%d drafts=%d close=%d reconnect=%d\n",
        independent, cancelled, selected, own, reading, bounded, frozen, drafts, retired, reconnect);
    std::printf("[sync-ui] apply/cancel/indicator=%d key-routing=%d stop-all=%d\n", sync_indicator, sync_routed, sync_stopped);
    return independent && cancelled && selected && own && reading && bounded && frozen && drafts && retired && reconnect
        && sync_indicator && sync_routed && sync_stopped;
}
