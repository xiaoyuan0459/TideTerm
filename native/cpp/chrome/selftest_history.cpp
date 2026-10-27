// Qt owns dwell/cancellation, real scrollbar input and the source window lifetime.
#include "cpp/chrome/selftest_history.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QDialog>
#include <QElapsedTimer>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMouseEvent>
#include <QScrollBar>
#include <QStyle>
#include <QStyleOptionSlider>
#include <QThread>
#include <cstdio>

namespace {
bool wait_until(const std::function<bool()> &ready) {
    QElapsedTimer clock; clock.start();
    while (!ready() && clock.elapsed() < 2000) {
        QApplication::processEvents(); QThread::msleep(5);
    }
    return ready();
}
void mouse(QWidget *target, QEvent::Type type, QPoint point, Qt::MouseButton button = Qt::NoButton,
           Qt::MouseButtons held = Qt::NoButton) {
    QMouseEvent event(type, QPointF(point), QPointF(target->mapToGlobal(point)), button, held, Qt::NoModifier);
    QApplication::sendEvent(target, &event);
}
void leave(QWidget *target) { QEvent event(QEvent::Leave); QApplication::sendEvent(target, &event); }
void key(QWidget *target, int code) {
    QKeyEvent event(QEvent::KeyPress, code, Qt::NoModifier);
    QApplication::sendEvent(target, &event);
}
QRect part(QScrollBar *bar, QStyle::SubControl control) {
    QStyleOptionSlider option; option.initFrom(bar); option.orientation = Qt::Vertical;
    option.minimum = bar->minimum(); option.maximum = bar->maximum();
    option.sliderValue = bar->value(); option.sliderPosition = bar->sliderPosition();
    option.pageStep = bar->pageStep(); option.singleStep = bar->singleStep();
    return bar->style()->subControlRect(QStyle::CC_ScrollBar, &option, control, bar);
}
void feed(uint32_t tab, const QByteArray &bytes) {
    term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
}
}

bool selftest_ui::verify_outline_hover(QDialog *dialog, PtyView *view, const QString &png) {
    auto *list = dialog->findChild<QListWidget *>(QStringLiteral("outlineList"));
    auto *filter = dialog->findChild<QLineEdit *>(QStringLiteral("outlineFilter"));
    auto *selected = dialog->findChild<QLabel *>(QStringLiteral("outlinePreview"));
    auto *hover = dialog->findChild<QLabel *>(QStringLiteral("hoverPreview"));
    if (!list || !filter || !selected || !hover) return false;
    filter->clear();
    QListWidgetItem *first = nullptr, *second = nullptr;
    for (int at = 0; at < list->count(); ++at) {
        auto *item = list->item(at);
        if (item->text().contains(QStringLiteral("FIRST"))) first = item;
        if (item->text().contains(QStringLiteral("SECOND"))) second = item;
    }
    if (!first || !second) return false;
    list->setCurrentItem(first);
    if (!activate_window(*dialog)) return false;
    const auto position = term_history_state(view->tab()).position;
    const QPoint point = list->visualItemRect(second).center();
    mouse(list->viewport(), QEvent::MouseMove, point);
    const bool delayed = !hover->isVisible();
    const bool opened = wait_until([&] { return hover->isVisible(); });
    bool ok = opened && delayed && hover->text().contains(QStringLiteral("SECOND output")) &&
        list->currentItem() == first && selected->text().startsWith(QStringLiteral("FIRST output")) &&
        term_history_state(view->tab()).position == position;
    if (!png.isEmpty()) ok &= hover->grab().save(png + QStringLiteral(".outline-hover.png"));
    mouse(list->viewport(), QEvent::MouseMove, QPoint(list->viewport()->width() / 2, list->viewport()->height() - 3));
    const bool blank = !hover->isVisible();
    leave(list->viewport()); const bool left = !hover->isVisible();
    mouse(list->viewport(), QEvent::MouseMove, list->visualItemRect(first).center());
    mouse(list->viewport(), QEvent::MouseMove, point);
    const bool reopened = wait_until([&] { return hover->isVisible(); });
    key(dialog, Qt::Key_Escape);
    const bool escaped = dialog->isVisible() && !hover->isVisible() && list->currentItem() == first;
    mouse(list->viewport(), QEvent::MouseMove, list->visualItemRect(first).center());
    mouse(list->viewport(), QEvent::MouseMove, point);
    const bool filtered_open = wait_until([&] { return hover->isVisible(); });
    filter->setText(QStringLiteral("FIRST"));
    const bool filtered = !hover->isVisible() && list->currentItem() == first;
    ok &= blank && left && reopened && escaped && filtered_open && filtered;
    std::printf("[commands] outline hover opened=%d delayed=%d blank=%d leave=%d reopen=%d Escape=%d filter-open=%d filter=%d all=%d\n",
        opened, delayed, blank, left, reopened, escaped, filtered_open, filtered, ok);
    return ok;
}

bool selftest_ui::verify_history_geometry(PtyView *view) {
    const auto *bar = view->findChild<QScrollBar *>(QStringLiteral("terminalHistoryBar"));
    const QPointF origin = view->grid_pt(QPointF());
    return bar && bar->geometry().right() == view->width() - 1 &&
        origin.x() < -chrome::metrics::kPanePadX &&
        view->grid_w() == bar->geometry().left() - chrome::metrics::kPanePadX + origin.x() &&
        view->grid_h() == view->height() - 2 * chrome::metrics::kPanePadY &&
        view->grid_pt(QPointF(100, 50)) == QPointF(100 + origin.x(), 50 - chrome::metrics::kPanePadY);
}

bool selftest_ui::verify_history_scroll(MainWindow &window, PtyView *view, const QString &png) {
    if (!activate_window(window)) return false;
    const uint32_t tab = view->tab();
    term_set_reading(tab, false); term_clear(tab, 2);
    QByteArray data;
    for (int row = 0; row < 100; ++row) data += QStringLiteral("HISTORY_%1\r\n").arg(row, 3, 10, QLatin1Char('0')).toUtf8();
    feed(tab, data); view->poll_and_update(false); view->repaint();
    auto *bar = view->findChild<QScrollBar *>(QStringLiteral("terminalHistoryBar"));
    auto *hover = view->findChild<QLabel *>(QStringLiteral("hoverPreview"));
    if (!bar || !hover || !bar->isVisible() || bar->maximum() <= 0) return false;
    view->setFocus();
    const auto before = term_history_state(tab);
    QRect groove = part(bar, QStyle::SC_ScrollBarGroove), thumb = part(bar, QStyle::SC_ScrollBarSlider);
    const QPoint top(bar->width() / 2, groove.top() + thumb.height() / 2);
    mouse(bar, QEvent::MouseMove, top);
    bool ok = wait_until([&] { return hover->isVisible(); });
    ok &= hover->text().contains(QStringLiteral("HISTORY_000")) &&
        term_history_state(tab).position == before.position && window.active_view() == view && view->hasFocus();
    if (!png.isEmpty()) {
        ok &= hover->grab().save(png + QStringLiteral(".history-hover.png"));
        ok &= window.grab().save(png + QStringLiteral(".history-bar.png"));
    }
    leave(bar); ok &= !hover->isVisible();
    const QPoint center = thumb.center();
    mouse(bar, QEvent::MouseButtonPress, center, Qt::LeftButton, Qt::LeftButton);
    mouse(bar, QEvent::MouseMove, top, Qt::NoButton, Qt::LeftButton);
    mouse(bar, QEvent::MouseButtonRelease, top, Qt::LeftButton);
    view->poll_and_update(false);
    const bool dragged = term_history_state(tab).position == 0 && term_scroll_offset(tab) > 0;
    key(bar, Qt::Key_End); view->poll_and_update(false);
    const bool ended = term_history_state(tab).position == before.maximum;
    mouse(bar, QEvent::MouseMove, part(bar, QStyle::SC_ScrollBarSlider).center());
    ok &= wait_until([&] { return hover->isVisible(); });
    feed(tab, QByteArray("\x1b[HCHANGED")); view->repaint();
    const bool updated = wait_until([&] { return hover->isVisible() && hover->text().contains(QStringLiteral("CHANGED")); });
    key(view, Qt::Key_Escape);
    const bool escaped = !hover->isVisible() && !term_reading(tab);
    mouse(bar, QEvent::MouseMove, part(bar, QStyle::SC_ScrollBarSlider).center());
    ok &= wait_until([&] { return hover->isVisible(); });
    QKeyEvent shortcut(QEvent::ShortcutOverride, Qt::Key_Left, Qt::ControlModifier | Qt::ShiftModifier);
    QApplication::sendEvent(view, &shortcut);
    const bool shortcut_hidden = !hover->isVisible();
    mouse(bar, QEvent::MouseMove, part(bar, QStyle::SC_ScrollBarSlider).center());
    ok &= wait_until([&] { return hover->isVisible(); });
    term_set_locked(true); view->repaint();
    const bool locked = !hover->isVisible();
    term_set_locked(false);
    term_clear(tab, 2); view->poll_and_update(false);
    const bool cleared = bar->isHidden();
    ok &= dragged && ended && updated && escaped && shortcut_hidden && locked && cleared;
    std::printf("[commands] history preview hover=%d drag=%d End=%d output=%d Escape=%d shortcut=%d lock=%d clear=%d\n",
        ok, dragged, ended, updated, escaped, shortcut_hidden, locked, cleared);
    return ok;
}
