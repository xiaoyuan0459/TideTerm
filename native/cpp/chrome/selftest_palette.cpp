// Actual filtering, keyboard dispatch and stable UI targets; line geometry belongs to Rust.
#include "cpp/chrome/selftest_palette.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QDialog>
#include <QLineEdit>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QTreeWidget>
#include <cmath>
#include <cstdio>

namespace {
void feed(uint32_t tab, const QByteArray &text) { term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(text.data()), size_t(text.size()))); }
void key(QWidget *widget, int code) {
    QKeyEvent press(QEvent::KeyPress, code, Qt::NoModifier); QApplication::sendEvent(widget, &press);
    QKeyEvent release(QEvent::KeyRelease, code, Qt::NoModifier); QApplication::sendEvent(widget, &release);
}
QString q(const rust::String &text) { return QString::fromUtf8(text.data(), int(text.size())); }
rust::Str rs(const QByteArray &text) { return rust::Str(text.constData(), size_t(text.size())); }
bool edit(MainWindow &window, const char *tid, const std::function<bool(QDialog *, QLineEdit *, QTreeWidget *)> &operation) {
    auto *action = chrome::find_menu_action(window.menuBar(), QLatin1String(tid)); if (!action || !action->isEnabled()) return false;
    bool result = false; QObject guard;
    QTimer::singleShot(0, &guard, [&] {
        auto *dialog = window.findChild<QDialog *>(QStringLiteral("commandPalette")); if (!dialog) return;
        result = operation(dialog, dialog->findChild<QLineEdit *>(QStringLiteral("paletteQuery")), dialog->findChild<QTreeWidget *>(QStringLiteral("paletteItems")));
        if (dialog->isVisible()) dialog->reject();
    });
    action->trigger(); return result;
}
void close_owned(MainWindow &window, uint32_t tab) {
    term_disconnect_tab(tab);
    for (int i = 0; i < window.page_count(); ++i) if (window.page_id(i) == tab) { window.close_tab(i); return; }
}
}

bool selftest_ui::verify_palette(MainWindow &window, PtyView *view, const QString &png) {
    if (!activate_window(window)) return false;
    const uint32_t tab = view->tab(); const auto uid = term_uid(tab);
    auto *draft = window.page_sendbar(window.current_page())->input(); draft->setText(QStringLiteral("PALETTE_DRAFT"));
    const auto fixture = [&] {
        term_set_reading(tab, false); term_clear(tab, 2);
        QByteArray bytes("\x1b]133;A\007$ \x1b]133;B\007echo PALETTE\r\n\x1b]133;C\007");
        for (int n = 0; n < 12; ++n) bytes += QStringLiteral("PALETTE_LINE_%1\r\n").arg(n, 2, 10, QLatin1Char('0')).toUtf8();
        bytes += "\x1b]133;D;0\007\x1b]133;A\007$ "; feed(tab, bytes); window.refresh_chrome();
    };
    fixture(); const auto offset = term_scroll_offset(tab);
    auto *toggle = chrome::find_menu_action(window.menuBar(), QStringLiteral("view:highlight-caret-line"));
    if (!toggle) return false; const bool checked = toggle->isChecked();
    bool ok = edit(window, "view:command-palette", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral(">")); auto *first = tree->currentItem(); key(input, Qt::Key_Down);
        bool pass = tree->currentItem() && tree->currentItem() != first;
        input->setText(QStringLiteral(">view:highlight-caret-line")); pass &= tree->topLevelItemCount() == 1;
        if (!png.isEmpty()) pass &= dialog->grab().save(png + QStringLiteral(".palette-actions.png"));
        key(input, Qt::Key_Return); key(input, Qt::Key_Return); return pass;
    });
    ok &= toggle->isChecked() != checked && window.property("highlightCaretLine").toBool() == toggle->isChecked();
    std::printf("[palette] action-once=%d\n", ok);
    const bool after = toggle->isChecked();
    ok &= edit(window, "view:command-palette", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *) {
        input->setText(QStringLiteral(">view:highlight-caret-line")); term_set_locked(true); key(input, Qt::Key_Return);
        const bool pass = dialog->isVisible() && toggle->isChecked() == after; term_set_locked(false); key(input, Qt::Key_Escape); return pass;
    });
    ok &= edit(window, "view:command-palette", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *) {
        input->setText(QStringLiteral(">view:highlight-caret-line")); toggle->setEnabled(false); key(input, Qt::Key_Return);
        const bool pass = dialog->isVisible() && toggle->isChecked() == after; toggle->setEnabled(true); key(input, Qt::Key_Escape); return pass;
    });
    ok &= edit(window, "view:command-palette", [&](QDialog *, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral(">no_such_action_b3a_fixture")); const bool pass = tree->topLevelItemCount() == 0;
        key(input, Qt::Key_Escape); return pass;
    });
    ok &= !term_reading(tab) && term_scroll_offset(tab) == offset && draft->text() == QStringLiteral("PALETTE_DRAFT");
    std::printf("[palette] disabled/cancel/draft=%d\n", ok);
    ok &= edit(window, "goto:position", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral(":99999999999999999999")); key(input, Qt::Key_Return);
        bool pass = dialog->isVisible() && !dialog->findChild<QPushButton *>(QStringLiteral("paletteGo"))->isEnabled();
        input->setText(QStringLiteral(":5")); pass &= tree->topLevelItemCount() == 1 && tree->topLevelItem(0)->text(1).contains(QStringLiteral("PALETTE_LINE_03"));
        pass &= !term_reading(tab) && term_scroll_offset(tab) == offset;
        if (!png.isEmpty()) pass &= dialog->grab().save(png + QStringLiteral(".palette-line.png"));
        key(input, Qt::Key_Return); return pass;
    });
    const Rect caret = term_caret_rect(tab); ok &= term_reading(tab) && std::abs(caret.y / caret.h - 4) < 0.1;
    std::printf("[palette] line=%d caret-row=%.2f reading=%d\n", ok, caret.y / caret.h, term_reading(tab));
    fixture();
    ok &= edit(window, "goto:position", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *) {
        input->setText(QStringLiteral(":5")); feed(tab, "\x1b[2J"); key(input, Qt::Key_Return);
        const bool pass = dialog->isVisible() && !term_reading(tab); key(input, Qt::Key_Escape); return pass;
    });
    fixture();
    ok &= edit(window, "goto:symbol", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral("@PALETTE")); const bool pass = tree->topLevelItemCount() == 1;
        if (!png.isEmpty()) dialog->grab().save(png + QStringLiteral(".palette-symbol.png"));
        key(input, Qt::Key_Return); return pass;
    });
    ok &= term_reading(tab) && term_uid(tab) == uid;
    std::printf("[palette] stale-line/symbol=%d\n", ok);
    // Filter the actual visible name; configured names intentionally take priority over OSC.
    auto cfg = sess_form_get(rust::Str(""), 0); cfg.name = rust::String("PaletteWindowTwo"); cfg.group_path = rust::String("Palette > Test");
#ifdef Q_OS_WIN
    cfg.local_shell = rust::String("pwsh"); cfg.local_args = rust::String("-NoProfile -NonInteractive");
#else
    cfg.local_shell = rust::String("sh"); cfg.local_args = rust::String("");
#endif
    const uint32_t second = sess_open_draft(cfg); if (second == UINT32_MAX) return false;
    window.add_tab_with(second); term_disconnect_tab(second);
    const uint32_t count = term_tab_count();
    ok &= edit(window, "goto:session", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral("tm ") + q(term_title(tab))); bool pass = tree->topLevelItemCount() == 1;
        if (!png.isEmpty()) pass &= dialog->grab().save(png + QStringLiteral(".palette-session.png"));
        key(input, Qt::Key_Return); return pass;
    });
    ok &= window.active_view() == view && term_tab_count() == count; close_owned(window, second);
    std::printf("[palette] live-switch=%d\n", ok);
    cfg.name = rust::String("B3A_saved_fixture");
    const auto saved = sess_save(cfg);
    if (saved.empty()) return false;
    const int pages = window.page_count();
    ok &= edit(window, "session:open", [&](QDialog *, QLineEdit *input, QTreeWidget *tree) {
        input->setText(QStringLiteral("tm B3A_saved_fixture")); const bool pass = tree->topLevelItemCount() == 1; key(input, Qt::Key_Return); return pass;
    });
    auto *opened = window.active_view(); const uint32_t opened_tab = opened ? opened->tab() : UINT32_MAX;
    const bool new_connection = opened && opened != view && window.page_count() == pages + 1;
    ok &= new_connection;
    std::printf("[palette] saved-open=%d new-connection=%d pages=%d/%d\n", ok, new_connection, window.page_count(), pages + 1);
    if (new_connection) {
        term_disconnect_tab(opened_tab); window.focus_session(tab); const uint32_t before = term_tab_count();
        ok &= edit(window, "goto:session", [&](QDialog *, QLineEdit *input, QTreeWidget *tree) { input->setText(QStringLiteral("tm B3A_saved_fixture")); const bool pass = tree->topLevelItemCount() == 1; key(input, Qt::Key_Return); return pass; });
        ok &= window.active_view() == opened && term_tab_count() == before; close_owned(window, opened_tab);
    }
    window.focus_session(tab);
    ok &= edit(window, "goto:session", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *) {
        input->setText(QStringLiteral("tm B3A_saved_fixture")); const int before = window.page_count(); sess_delete(saved); key(input, Qt::Key_Return);
        const bool pass = dialog->isVisible() && window.page_count() == before; key(input, Qt::Key_Escape); return pass;
    });
    ok &= draft->text() == QStringLiteral("PALETTE_DRAFT");
    std::printf("[commands] palette filter/keyboard/action-once/line/symbol/live/saved/cancel/stale: %d\n", ok);
    MainWindow other; other.show(); auto *retiring = other.active_view(); term_disconnect_tab(retiring->tab());
    bool retired = edit(other, "view:command-palette", [&](QDialog *dialog, QLineEdit *input, QTreeWidget *) {
        input->setText(QStringLiteral(">view:highlight-caret-line")); auto *action = chrome::find_menu_action(other.menuBar(), QStringLiteral("view:highlight-caret-line")); const bool before = action->isChecked();
        other.close_current_tab(); key(input, Qt::Key_Return); const bool pass = dialog->isVisible() && action->isChecked() == before; key(input, Qt::Key_Escape); return pass;
    });
    std::printf("[commands] palette rejects retired action origin: %d\n", retired);
    window.activateWindow(); view->setFocus(); return ok && retired;
}
