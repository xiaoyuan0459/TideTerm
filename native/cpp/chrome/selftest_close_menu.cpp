// Public window lifecycle and real dialog persistence; no injected close policy.
#include "cpp/chrome/selftest_close_menu.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/prefs_dialog.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QAction>
#include <QCheckBox>
#include <QDialog>
#include <QLineEdit>
#include <QKeyEvent>
#include <QMenuBar>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QTimer>
#include <QToolButton>
#include <cstdio>
#include <functional>

namespace {
struct Answer { bool seen = false, safe_default = false, checkbox = false, saved = true; };
Answer answer(MainWindow &window, const std::function<void()> &invoke, bool yes,
              bool remember = false, const QString &png = {}, bool enter = false) {
    Answer result;
    QTimer response;
    response.setSingleShot(true);
    QObject::connect(&response, &QTimer::timeout, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        auto *box = dynamic_cast<QMessageBox *>(dialog);
        if (!box || box->parentWidget() != &window) { if (dialog) dialog->reject(); return; }
        result.seen = true;
        result.safe_default = box->defaultButton() == box->button(QMessageBox::No);
        result.checkbox = box->checkBox() != nullptr;
        if (!png.isEmpty()) result.saved = box->grab().save(png + QStringLiteral(".close-confirm.png"));
        if (box->checkBox()) box->checkBox()->setChecked(remember);
        if (enter) {
            QKeyEvent key(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
            QApplication::sendEvent(box, &key);
        } else box->button(yes ? QMessageBox::Yes : QMessageBox::No)->click();
    });
    response.start(30);
    invoke();
    return result;
}
bool preferences(MainWindow &window, bool accept) {
    bool found = false, initially_off = false;
    QTimer response;
    response.setSingleShot(true);
    QObject::connect(&response, &QTimer::timeout, [&] {
        auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        auto *check = dialog ? dialog->findChild<QCheckBox *>(QStringLiteral("confirmWindowClose")) : nullptr;
        found = check != nullptr;
        if (check) { initially_off = !check->isChecked(); check->setChecked(true); }
        if (dialog) { if (accept && check) dialog->accept(); else dialog->reject(); }
    });
    response.start(30);
    const bool saved = run_prefs_dialog(&window, 0);
    return found && initially_off && saved == accept;
}
}

bool selftest_ui::verify_window_close(MainWindow &window, const QString &png) {
    const uint32_t before = term_tab_count();
    const uint32_t tab = window.page_id(0);
    const auto uid = term_uid(tab);
    auto *draft = window.page_sendbar(window.current_page())->input();
    const QString text = draft->text();
    const auto preserved = [&] {
        return window.isVisible() && term_tab_count() == before &&
            !window.pane_view(0, 0)->is_closed() && term_uid(tab) == uid && draft->text() == text;
    };
    const Answer first = answer(window, [&] { window.close(); }, false, true, png, true);
    bool ok = first.seen && first.safe_default && first.checkbox && first.saved && preserved();
    std::printf("[close] native close cancel preserves sessions/draft/default/checkbox: %d\n", ok);
    if (!ok) return false; // The unfixed window already retired its sessions.
    auto *button = window.findChild<QToolButton *>(QStringLiteral("windowClose"));
    if (!button) return false;
    const Answer caption = answer(window, [&] { button->click(); }, false);
    ok &= caption.seen && caption.safe_default && preserved();
    for (const char *id : {"session:close-window", "session:exit"}) {
        auto *action = chrome::find_menu_action(window.menuBar(), QLatin1String(id));
        if (!action) return false;
        const Answer menu = answer(window, [&] { action->trigger(); }, false);
        ok &= menu.seen && menu.safe_default && preserved();
    }
    std::printf("[close] caption/menu/exit cancel and cancelled opt-out: %d\n", ok);
    const Answer confirmed = answer(window, [&] { window.close(); }, true, true);
    ok &= confirmed.seen && !window.isVisible();
    return ok;
}

bool selftest_ui::verify_close_preference(MainWindow &survivor) {
    MainWindow settings(false);
    settings.show(); // Keep a window alive while testing the opt-out in another window.
    const bool cancelled = preferences(settings, false);
    const Answer quiet = answer(survivor, [&] { survivor.close(); }, true);
    const bool remembered = !quiet.seen && !survivor.isVisible() && term_tab_count() == 0;
    const bool restored = preferences(settings, true);
    settings.add_tab();
    const Answer again = answer(settings, [&] { settings.close(); }, false);
    const bool reminder = again.seen && settings.isVisible() && term_tab_count() == 1;
    const Answer final = answer(settings, [&] { settings.close(); }, true);
    const bool ok = cancelled && remembered && restored && reminder && final.seen && term_tab_count() == 0;
    std::printf("[close] opt-out cross-window / preferences cancel / restore reminder: %d/%d/%d/%d\n",
        remembered, cancelled, restored, reminder);
    return ok;
}

bool selftest_ui::accept_window_close(MainWindow &window) {
    const Answer confirmed = answer(window, [&] { window.close(); }, true);
    return confirmed.seen && !window.isVisible();
}

bool selftest_ui::capture_main_menus(MainWindow &window, const QString &png) {
    if (png.isEmpty()) return true;
    bool ok = true; int index = 0;
    for (auto *action : window.menuBar()->actions()) {
        auto *menu = action->menu(); if (!menu) continue;
        menu->popup(window.menuBar()->mapToGlobal(window.menuBar()->actionGeometry(action).bottomLeft()));
        QApplication::processEvents();
        const QString path = png + QStringLiteral(".menu-%1.png").arg(index++);
        ok &= menu->grab().save(path);
        menu->hide(); QApplication::processEvents();
    }
    std::printf("[menus] real main-menu captures=%d saved=%d\n", index, ok);
    return ok;
}
