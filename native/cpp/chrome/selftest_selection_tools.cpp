// This boundary owns actual QAction and pointer delivery, not range algorithms.
#include "cpp/chrome/selftest_selection_tools.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QMenuBar>
#include <QMouseEvent>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
void drag(PtyView *view, QPointF start, QPointF end) {
    QMouseEvent press(QEvent::MouseButtonPress, start, view->mapToGlobal(start), Qt::LeftButton, Qt::LeftButton, Qt::ShiftModifier);
    QMouseEvent move(QEvent::MouseMove, end, view->mapToGlobal(end), Qt::NoButton, Qt::LeftButton, Qt::ShiftModifier);
    QMouseEvent release(QEvent::MouseButtonRelease, end, view->mapToGlobal(end), Qt::LeftButton, Qt::NoButton, Qt::ShiftModifier);
    QApplication::sendEvent(view, &press); QApplication::sendEvent(view, &move); QApplication::sendEvent(view, &release);
}
void double_click(PtyView *view, QPointF point) {
    for (const auto type : {QEvent::MouseButtonPress, QEvent::MouseButtonRelease, QEvent::MouseButtonDblClick, QEvent::MouseButtonRelease}) {
        QMouseEvent event(type, point, view->mapToGlobal(point), Qt::LeftButton,
            type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::LeftButton, Qt::ShiftModifier);
        QApplication::sendEvent(view, &event);
    }
}
}

bool selftest_ui::verify_selection_tools(MainWindow &window, PtyView *view, const std::function<QPointF(int,int)> &at) {
    const auto tab = view->tab();
    const auto action = [&](const char *id) { return chrome::find_menu_action(window.menuBar(), QLatin1String(id)); };
    auto *lock = action("select:lock"); auto *split = action("select:split");
    auto *invert = action("select:invert"); auto *paths = action("select:path-components");
    if (!lock || !split || !invert || !paths) return false;
    term_deselect(tab); drag(view, at(2,0), at(4,1)); window.refresh_chrome();
    const QString original = q(term_selected_text(tab));
    lock->trigger(); drag(view, at(0,0), at(1,1));
    const bool locked = lock->isChecked() && term_selection_locked(tab)
        && !split->isEnabled() && !invert->isEnabled() && q(term_selected_text(tab)) == original;
    lock->trigger(); split->trigger();
    const bool split_ok = term_selection_status(tab).count == 2 && q(term_selected_text(tab)) == original;
    invert->trigger();
    const bool inverted = q(term_selected_text(tab)) == QStringLiteral("01\nFGHIJ");
    term_clear(tab, 2);
    const QByteArray text("C:\\repo\\child.txt");
    term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(text.data()), size_t(text.size())));
    view->repaint(); window.refresh_chrome();
    if (paths->isChecked()) paths->trigger();
    double_click(view, at(10,0));
    const bool full_path = q(term_selected_text(tab)) == QStringLiteral("C:\\repo\\child.txt");
    paths->trigger(); double_click(view, at(10,0));
    const bool component = paths->isChecked() && q(term_selected_text(tab)) == QStringLiteral("child.txt");
    lock->trigger(); term_clear(tab, 2); window.refresh_chrome();
    const bool retired = !term_selection_locked(tab) && !lock->isChecked() && !lock->isEnabled();
    if (paths->isChecked()) paths->trigger();
    std::printf("[selection-tools] menu-lock=%d split=%d visible-invert=%d whole-path=%d component=%d retirement=%d\n",
        locked, split_ok, inverted, full_path, component, retired);
    return locked && split_ok && inverted && full_path && component && retired;
}
