#include "cpp/clipboard.h"
#include "cpp/chrome/selftest_clipboard.h"
// Qt owns the editor/focus/stop journey; real shell output is the execution oracle.
#include "cpp/chrome/selftest_multiline.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/multiline_dialog.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QDir>
#include <QElapsedTimer>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QTemporaryDir>
#include <QThread>
#include <QToolButton>
#include <functional>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &predicate, int timeout = 5000) {
    QElapsedTimer timer; timer.start();
    do { term_pump(); QApplication::processEvents(); if (predicate()) return true; QThread::msleep(10); }
    while (timer.elapsed() < timeout);
    return false;
}
QDialog *editor(MainWindow &window) { return window.findChild<QDialog *>(QStringLiteral("multilineEditor")); }
void close_editor(QDialog *dialog) {
    dialog->reject(); QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
}
}

bool selftest_ui::verify_multiline(const QString &png) {
    std::printf("[multiline] begin\n");
    const QString root = QDir(QCoreApplication::applicationDirPath()).absoluteFilePath(QStringLiteral("../fixtures"));
    QDir().mkpath(root); QTemporaryDir fixture(root + QStringLiteral("/multiline-XXXXXX"));
    if (!fixture.isValid()) return false;
    MainWindow window(false);
    auto cfg = sess_form_get(rust::Str(""), 0);
    cfg.local_cwd = rust::String(fixture.path().toUtf8().toStdString());
#ifdef Q_OS_WIN
    // Reuse the launcher's isolated profile/history policy, including auxiliary shells.
    cfg.local_shell = rust::String("pwsh"); cfg.local_args = rust::String("");
    const QString once = QStringLiteral("Write-Output ('B3B_'+'ONCE')");
    const QString first = QStringLiteral("Write-Output ('B3B_'+'FIRST')");
    const QString second = QStringLiteral("Write-Output ('B3B_'+'SECOND')");
#else
    cfg.local_shell = rust::String("bash"); cfg.local_args = rust::String("");
    const QString once = QStringLiteral("printf '%s%s\\n' B3B_ ONCE");
    const QString first = QStringLiteral("printf '%s%s\\n' B3B_ FIRST");
    const QString second = QStringLiteral("printf '%s%s\\n' B3B_ SECOND");
#endif
    const uint32_t tab = sess_open_draft(cfg); if (tab == UINT32_MAX) return false;
    window.add_tab_with(tab); window.resize(1200, 780); window.show();
    auto *view = window.active_view();
    if (!activate_window(window) || !wait_for([&] {
        const QString screen = q(term_screen_text(tab)).trimmed();
        return screen.endsWith(QLatin1Char('>')) || screen.endsWith(QLatin1Char('$')) || screen.endsWith(QLatin1Char('#'));
    }, 12000)) return false;
    auto *line = window.page_sendbar(0)->input(); line->setText(QStringLiteral("SENDER_DRAFT"));
    host_clipboard::copy_text(QStringLiteral("alpha\tbeta\nlast"));
    chrome::find_menu_action(window.menuBar(), QStringLiteral("edit:paste"))->trigger();
    QApplication::processEvents(); auto *dialog = editor(window); if (!dialog) return false;
    auto *text = dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"));
    auto *tabs = dialog->findChild<QComboBox *>(QStringLiteral("multilineTabs"));
    auto *eol = dialog->findChild<QComboBox *>(QStringLiteral("multilineEol"));
    tabs->setCurrentIndex(2); eol->setCurrentIndex(2);
    const QString original = text->toPlainText();
    text->setPlainText(QString(65537, QLatin1Char('x')));
    const bool budget = text->toPlainText() == original
        && !dialog->findChild<QPushButton *>(QStringLiteral("multilineSend"))->isEnabled();
    const bool preview = dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilinePreview"))->toPlainText()
        == QStringLiteral("alpha    beta\\r\\n\nlast\n");
    text->insertPlainText(QStringLiteral("edited "));
    const QString draft = text->toPlainText();
    if (!png.isEmpty()) dialog->grab().save(png + QStringLiteral(".multiline.png"));
    close_editor(dialog);
    std::printf("[multiline] preview-cancel\n");
    auto *open = window.page_sendbar(0)->findChild<QToolButton *>(QStringLiteral("sendMultiline")); open->click();
    QApplication::processEvents(); dialog = editor(window); if (!dialog) return false;
    const bool preserved = dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"))->toPlainText() == draft
        && line->text() == QStringLiteral("SENDER_DRAFT");
    text = dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText")); text->setPlainText(once);
    dialog->findChild<QCheckBox *>(QStringLiteral("multilineAppend"))->setChecked(true);
    auto *send = dialog->findChild<QPushButton *>(QStringLiteral("multilineSend")); send->click(); send->click();
    const bool sent_once = wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("B3B_ONCE")); })
        && q(term_screen_text(tab)).count(QStringLiteral("B3B_ONCE")) == 1;
    close_editor(dialog);
    std::printf("[multiline] sent-once=%d\n", sent_once);
    term_set_reading(tab, true); term_select_all(tab); window.refresh_chrome();
    const QString selected = q(term_selected_text(tab));
    host_clipboard::copy_text(QStringLiteral("CLIPBOARD_SENTINEL"));
    auto *selected_action = chrome::find_menu_action(window.menuBar(), QStringLiteral("edit:paste-selected"));
    if (!selected_action) { std::printf("[multiline] missing selection paste action\n"); return false; }
    selected_action->trigger();
    QApplication::processEvents(); dialog = editor(window); if (!dialog) return false;
    const bool selected_text = selected.contains(QLatin1Char('\n'))
        && dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"))->toPlainText() == selected;
    close_editor(dialog);
    const bool selected_cancel = selected_text && term_reading(tab)
        && selftest_ui::clipboard_matches(QStringLiteral("CLIPBOARD_SENTINEL"));
    term_set_reading(tab, false); term_deselect(tab); view->sync_reading_mode();
    std::printf("[multiline] selected-cancel=%d\n", selected_cancel);
    const uint32_t auxiliary_tab = sess_open_draft(cfg);
    bool auxiliary_route = false;
    bool window_all = false;
    if (auxiliary_tab != UINT32_MAX) {
        // A real PtyView outside page_roots_, like a docked auxiliary shell.
        PtyView auxiliary(auxiliary_tab, true); auxiliary.setParent(&window);
        const bool prompt = wait_for([&] {
            const QString screen = q(term_screen_text(auxiliary_tab)).trimmed();
            return screen.endsWith(QLatin1Char('>')) || screen.endsWith(QLatin1Char('$')) || screen.endsWith(QLatin1Char('#'));
        }, 12000);
#ifdef Q_OS_WIN
        const QString auxiliary_command = QStringLiteral("Write-Output ('B3B_'+'AUX')\n");
#else
        const QString auxiliary_command = QStringLiteral("printf '%s%s\\n' B3B_ AUX\n");
#endif
        host_clipboard::copy_text(auxiliary_command);
        chrome::paste_clipboard(&auxiliary); QApplication::processEvents();
        if (auto *auxiliary_editor = editor(window)) {
            auxiliary_editor->findChild<QCheckBox *>(QStringLiteral("multilinePaced"))->setChecked(true);
            std::printf("[multiline] auxiliary prompt=%d enabled=%d\n", prompt,
                auxiliary_editor->findChild<QPushButton *>(QStringLiteral("multilineSend"))->isEnabled());
            auxiliary_editor->findChild<QPushButton *>(QStringLiteral("multilineSend"))->click();
            auxiliary_route = prompt && wait_for([&] { return q(term_screen_text(auxiliary_tab)).contains(QStringLiteral("B3B_AUX")); })
                && !q(term_screen_text(tab)).contains(QStringLiteral("B3B_AUX"));
            close_editor(auxiliary_editor);
        }
        window.page_sendbar(0)->findChild<QComboBox *>(QStringLiteral("sendTarget"))->setCurrentIndex(2);
        open->click(); QApplication::processEvents();
        if (auto *all_editor = editor(window)) {
#ifdef Q_OS_WIN
            const QString all_command = QStringLiteral("Write-Output ('B3B_'+'ALL')");
#else
            const QString all_command = QStringLiteral("printf '%s%s\\n' B3B_ ALL");
#endif
            all_editor->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"))->setPlainText(all_command);
            all_editor->findChild<QCheckBox *>(QStringLiteral("multilineAppend"))->setChecked(true);
            all_editor->findChild<QPushButton *>(QStringLiteral("multilineSend"))->click();
            window_all = wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("B3B_ALL"))
                && q(term_screen_text(auxiliary_tab)).contains(QStringLiteral("B3B_ALL")); });
            close_editor(all_editor);
        }
        auxiliary.mark_closed(); term_close_tab(auxiliary_tab);
    }
    std::printf("[multiline] auxiliary-current=%d window-all=%d\n", auxiliary_route, window_all);
    open->click(); QApplication::processEvents(); dialog = editor(window); if (!dialog) return false;
    dialog->findChild<QPlainTextEdit *>(QStringLiteral("multilineText"))->setPlainText(first + QLatin1Char('\n') + second);
    dialog->findChild<QCheckBox *>(QStringLiteral("multilineAppend"))->setChecked(true);
    dialog->findChild<QCheckBox *>(QStringLiteral("multilinePaced"))->setChecked(true);
    dialog->findChild<QSpinBox *>(QStringLiteral("multilineInterval"))->setValue(1000);
    dialog->findChild<QPushButton *>(QStringLiteral("multilineSend"))->click();
    const uint32_t other = sess_open_draft(cfg); window.add_tab_with(other);
    dialog->findChild<QPushButton *>(QStringLiteral("multilineStop"))->click();
    const bool first_sent = wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("B3B_FIRST")); });
    wait_for([] { return false; }, 1150);
    const bool stopped = first_sent && !q(term_screen_text(tab)).contains(QStringLiteral("B3B_SECOND"))
        && !q(term_screen_text(other)).contains(QStringLiteral("B3B_FIRST"));
    close_editor(dialog);
    std::printf("[multiline] stopped=%d\n", stopped);
    term_disconnect_tab(other);
    window.focus_session(tab);
    window.page_sendbar(0)->findChild<QComboBox *>(QStringLiteral("sendTarget"))->setCurrentIndex(2);
    open->click(); QApplication::processEvents(); dialog = editor(window);
    if (!dialog) return false;
    const bool rejected_group = !dialog->findChild<QPushButton *>(QStringLiteral("multilineSend"))->isEnabled();
    wait_for([] { return false; }, 150);
    dialog->findChild<QComboBox *>(QStringLiteral("multilineTarget"))->setCurrentIndex(0);
    const bool recovered = rejected_group && dialog->findChild<QPushButton *>(QStringLiteral("multilineSend"))->isEnabled();
    std::printf("[multiline] scope-recovered=%d\n", recovered);
    window.close_tab(0);
    const bool retired = wait_for([&] { return !dialog->findChild<QPushButton *>(QStringLiteral("multilineSend"))->isEnabled(); }, 1000);
    close_editor(dialog); QApplication::clipboard()->clear();
    std::printf("[multiline] preview=%d budget=%d cancel-draft=%d shell-once=%d selected-cancel=%d auxiliary=%d focus-stop=%d scope-recovery=%d source-close=%d\n",
        preview, budget, preserved, sent_once, selected_cancel, auxiliary_route, stopped, recovered, retired);
    return preview && budget && preserved && sent_once && selected_cancel && auxiliary_route && window_all && stopped && recovered && retired;
}
