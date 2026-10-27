#include "cpp/chrome/selftest_command_templates.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_completion.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QElapsedTimer>
#include <QFontMetrics>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
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
void key(QWidget *widget, int code, Qt::KeyboardModifiers mods = Qt::NoModifier, const QString &text = {}) {
    QKeyEvent event(QEvent::KeyPress, code, mods, text); QApplication::sendEvent(widget, &event);
}
void type(QLineEdit *edit, const QString &text) {
    edit->selectAll(); key(edit, Qt::Key_Backspace); key(edit, 0, Qt::NoModifier, text);
}
bool wait_for(const std::function<bool()> &predicate, int ms = 5000) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); QApplication::processEvents(); if (predicate()) return true; QThread::msleep(10); } while (timer.elapsed() < ms);
    return false;
}
}

bool selftest_ui::verify_command_templates(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
    auto *input = bar->input();
    auto *popup = bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    auto *manage = bar->findChild<QToolButton *>(QStringLiteral("sendManageCommands"));
    if (!popup || !manage || !activate_window(window)) return false;
    bool ok = true;
    const auto before = bar->sent_count();
#ifdef Q_OS_WIN
    const QString vocabulary_query=QStringLiteral("Get-Ch");
    const QString vocabulary_command=QStringLiteral("Get-ChildItem");
#else
    const QString vocabulary_query=QStringLiteral("pw");
    const QString vocabulary_command=QStringLiteral("pwd");
#endif
    input->setFocus(); input->setText(vocabulary_query); key(input,Qt::Key_Space,Qt::ControlModifier);
    int vocabulary_row=-1;
    for(int row=0;row<popup->count();++row) if(popup->item(row)->data(Qt::UserRole+1).toInt()==13 && popup->item(row)->data(Qt::UserRole).toString()==vocabulary_command) vocabulary_row=row;
    const bool reference=vocabulary_row>=0 && popup->isVisible() && popup->item(vocabulary_row)->toolTip().contains(QStringLiteral("https://"));
    if(!reference) return false;
    popup->setCurrentRow(vocabulary_row); key(input,Qt::Key_Tab);
    const bool vocabulary_fill=input->text()==vocabulary_command && bar->sent_count()==before;
    std::printf("[vocabulary] context-filtered reference/source/accept-only=%d\n",vocabulary_fill);
    if(!vocabulary_fill) return false;
    input->setFocus(); input->setText(QStringLiteral("docker logs --ta"));
    key(input, Qt::Key_Space, Qt::ControlModifier);
    const bool rule = popup->isVisible() && popup->count() == 1
        && popup->item(0)->toolTip().contains(QStringLiteral("docs.docker.com/reference/cli/docker/container/logs/"));
    if (!rule) { std::printf("[templates] rule-visible=%d rows=%d shell=%u\n",popup->isVisible(),popup->count(),unsigned(completion_context(tab).shell_kind)); return false; }
    key(input, Qt::Key_Tab);
    ok &= input->text() == QStringLiteral("docker logs --tail ") && bar->sent_count() == before;
    std::printf("[templates] offline structural rule/source/draft only: %d\n", ok);
    auto modal = [&](const std::function<void(QDialog *)> &action, const std::function<void()> &open) {
        QTimer dispatch; dispatch.setSingleShot(true);
        QObject::connect(&dispatch, &QTimer::timeout, &window, [&, action] {
            auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog) { ok = false; return; }
            action(dialog);
            if (dialog->isVisible()) { ok = false; dialog->reject(); }
        }); dispatch.start(0); open();
        if (dispatch.isActive()) { dispatch.stop(); ok = false; std::printf("[templates] expected editor did not open\n"); }
    };
#ifdef Q_OS_WIN
    const QString text = QStringLiteral("Write-Output ('B3C2_'+'{{value}}')");
    const QString expected = QStringLiteral("Write-Output ('B3C2_'+'ONCE')");
#else
    const QString text = QStringLiteral("printf '%s%s\\n' B3C2_ {{value}}");
    const QString expected = QStringLiteral("printf '%s%s\\n' B3C2_ ONCE");
#endif
    if (!wait_for([&] { return manage->isEnabled() && completion_context(tab).ready; })) {
        std::printf("[templates] editor unavailable enabled=%d ready=%d reading=%d active=%d\n", manage->isEnabled(),
            completion_context(tab).ready, term_reading(tab), window.isActiveWindow()); return false;
    }
    modal([&](QDialog *dialog) {
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandAdd"))->click();
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandGroup")), QStringLiteral("Ops"));
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandName")), QStringLiteral("Template"));
        type(dialog->findChild<QLineEdit *>(QStringLiteral("userCommandText")), text);
        dialog->findChild<QCheckBox *>(QStringLiteral("userCommandTemplate"))->setChecked(true);
        dialog->findChild<QPushButton *>(QStringLiteral("userCommandSave"))->click();
    }, [&] { manage->click(); });
    auto catalog = command_catalog();
    if (catalog.entries.size() != 1 || !catalog.entries[0].is_template) return false;
    const auto id = catalog.entries[0].id;
    const UserCommand template_entry = catalog.entries[0];
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    auto *button = bar->findChild<QToolButton *>(QStringLiteral("quickCommand_%1").arg(id));
    if (!button || !wait_for([&] { return button->isVisible(); })) return false;
    input->setText(QStringLiteral("KEEP_TEMPLATE_DRAFT"));
    modal([&](QDialog *dialog) {
        dialog->findChild<QLineEdit *>(QStringLiteral("templateParam_value"))->setText(QStringLiteral("CANCEL"));
        dialog->reject();
    }, [&] { button->click(); });
    ok &= input->text() == QStringLiteral("KEEP_TEMPLATE_DRAFT") && bar->sent_count() == before;
    modal([&](QDialog *dialog) {
        auto *apply = dialog->findChild<QPushButton *>(QStringLiteral("templateApply"));
        ok &= !apply->isEnabled();
        dialog->findChild<QLineEdit *>(QStringLiteral("templateParam_value"))->setText(QStringLiteral("ONCE"));
        ok &= dialog->findChild<QPlainTextEdit *>(QStringLiteral("templatePreview"))->toPlainText() == expected;
        if (!png.isEmpty()) ok &= dialog->grab().save(png + QStringLiteral(".template-editor.png"));
        apply->click();
    }, [&] { button->click(); });
    ok &= input->text() == expected && bar->sent_count() == before;
    QKeyEvent residual(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QString(), true, 1);
    QApplication::sendEvent(input, &residual);
    const bool residual_silent = bar->sent_count() == before;
    std::printf("[templates] residual Enter after draft fill never sends: %d\n", residual_silent);
    if (!residual_silent) return false;
    key(input, Qt::Key_End);
    modal([&](QDialog *dialog) {
        dialog->findChild<QLineEdit *>(QStringLiteral("templateParam_value"))->setText(QStringLiteral("ONCE"));
        dialog->findChild<QPushButton *>(QStringLiteral("templateApply"))->click();
    }, [&] { button->click(); });
    QApplication::sendEvent(input, &residual);
    const bool unchanged_silent = input->text() == expected && bar->sent_count() == before;
    std::printf("[templates] accepting identical draft still rejects residual Enter: %d\n", unchanged_silent);
    if (!unchanged_silent) return false;
    if (!activate_window(window)) return false;
    input->setFocus(); key(input, Qt::Key_Return);
    ok &= wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("B3C2_ONCE")); });
    ok &= bar->sent_count() == before + 1 && q(term_screen_text(tab)).count(QStringLiteral("B3C2_ONCE")) == 1;
    std::printf("[templates] explicit template save/QuickBar/cancel/preview/draft/send once: %d\n", ok);
    const QString draft = input->text();
    modal([&](QDialog *dialog) {
        term_set_reading(tab, true);
        dialog->findChild<QLineEdit *>(QStringLiteral("templateParam_value"))->setText(QStringLiteral("LATE"));
        ok &= !dialog->findChild<QPushButton *>(QStringLiteral("templateApply"))->isEnabled();
        dialog->reject();
    }, [&] { button->click(); });
    term_set_reading(tab, false);
    ok &= input->text() == draft && bar->sent_count() == before + 1;
    // Populate a second group and an overflow through the production catalog boundary.
    catalog = command_catalog();
    for (uint32_t n = 2; n <= 12; ++n) {
        UserCommand entry; entry.id = n; entry.group = n == 12 ? "Other" : "Ops";
        entry.name = rust::String(QStringLiteral("Command %1").arg(n, 2, 10, QLatin1Char('0')).toStdString());
        entry.command = rust::String(QStringLiteral("echo QUICK_%1").arg(n).toStdString()); catalog.entries.push_back(std::move(entry));
    }
    ok &= command_catalog_save(catalog.revision, rust::Slice<const UserCommand>(catalog.entries.data(), catalog.entries.size())) == 0;
    chrome::refresh_send_quickbar(bar);
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    auto *groups = bar->findChild<QComboBox *>(QStringLiteral("quickCommandGroup"));
    if (!groups) return false;
    groups->setCurrentIndex(groups->findData(QStringLiteral("Other")));
    auto *other = bar->findChild<QToolButton *>(QStringLiteral("quickCommand_12"));
    if (!other || !wait_for([&] { return other->isVisible(); })) return false;
    other->click(); ok &= input->text() == QStringLiteral("echo QUICK_12");
    groups->setCurrentIndex(groups->findData(QStringLiteral("Ops")));
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QApplication::processEvents();
    auto *more = bar->findChild<QToolButton *>(QStringLiteral("quickCommandMore"));
    if (!more || !more->menu() || more->menu()->actions().isEmpty()) return false;
    const auto visible = bar->findChildren<QToolButton *>();
    bool readable = true; int labels = 0;
    for (auto *left : visible) {
        if (!left->isVisible() || !left->objectName().startsWith(QStringLiteral("quickCommand_"))) continue;
        ++labels; readable &= left->width() >= left->fontMetrics().horizontalAdvance(left->text());
        for (auto *right : visible) if (left != right && right->isVisible()
            && right->objectName().startsWith(QStringLiteral("quickCommand_"))) readable &= !left->geometry().intersects(right->geometry());
    }
    readable &= labels >= 2;
    std::printf("[templates] QuickBar short labels readable/non-overlap: %d\n", readable);
    ok &= readable;
    QAction *overflow = more->menu()->actions().first();
    overflow->trigger(); ok &= input->text() == QStringLiteral("echo QUICK_10");
    if (!png.isEmpty()) ok &= window.grab().save(png + QStringLiteral(".quickbar.png"));
    input->setText(QStringLiteral("KEEP_STALE_ACTION"));
    catalog = command_catalog();
    ok &= command_catalog_save(catalog.revision, {}) == 0;
    overflow->trigger();
    ok &= input->text() == QStringLiteral("KEEP_STALE_ACTION") && bar->sent_count() == before + 1;
    chrome::refresh_send_quickbar(bar);
    std::printf("[templates] reading retirement/groups/overflow/stale action: %d\n", ok);
    catalog = command_catalog();
    ok &= command_catalog_save(catalog.revision, rust::Slice<const UserCommand>(&template_entry, 1)) == 0;
    chrome::refresh_send_quickbar(bar);
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    auto *closing = bar->findChild<QToolButton *>(QStringLiteral("quickCommand_%1").arg(id));
    if (!closing || !wait_for([&] { return closing->isVisible(); })) return false;
    QPointer<QLineEdit> retired(input);
    modal([&](QDialog *dialog) {
        window.close_tab(window.current_page());
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        dialog->findChild<QLineEdit *>(QStringLiteral("templateParam_value"))->setText(QStringLiteral("LATE"));
        ok &= retired.isNull() && !dialog->findChild<QPushButton *>(QStringLiteral("templateApply"))->isEnabled()
            && dialog->findChild<QPlainTextEdit *>(QStringLiteral("templatePreview"))->toPlainText().isEmpty();
        dialog->reject();
    }, [&] { closing->click(); });
    std::printf("[templates] source close retires editor and sender safely: %d\n", ok);
    catalog = command_catalog();
    ok &= command_catalog_save(catalog.revision, {}) == 0;
    return ok;
}
