// C3 owns panel drag gestures, preview, cancellation and resulting dock geometry.
#include "cpp/chrome/selftest_groups.h"
#include "cpp/chrome/docks.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QDockWidget>
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QScreen>
#include <QThread>
#include <cstdio>

namespace {
void mouse(QWidget *header, QEvent::Type type, const QPoint &global,
           Qt::MouseButton held = Qt::LeftButton) {
    QMouseEvent event(type, header->mapFromGlobal(global), global,
        type == QEvent::MouseMove ? Qt::NoButton : held,
        type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::LeftButton, Qt::NoModifier);
    QApplication::sendEvent(header, &event);
    QApplication::processEvents();
}
void begin(QDockWidget *dock, const QPoint &destination) {
    auto *header = dock->titleBarWidget();
    mouse(header, QEvent::MouseButtonPress, header->mapToGlobal(QPoint(50, 10)));
    mouse(header, QEvent::MouseMove, destination);
}
bool visible_preview(QMainWindow *window) {
    auto *preview = window->findChild<QWidget *>(QStringLiteral("panelDropPreview"));
    return preview && preview->isVisible() && !preview->size().isEmpty();
}
void cancel(QWidget *header) {
    QKeyEvent escape(QEvent::KeyPress, Qt::Key_Escape, Qt::NoModifier);
    QApplication::sendEvent(header, &escape);
    QApplication::processEvents();
}

bool settle_frame(QDockWidget *dock) {
    QElapsedTimer deadline, quiet;
    deadline.start();
    quiet.start();
    QRect previous = dock->frameGeometry();
    while (deadline.elapsed() < 2000) {
        QGuiApplication::sync();
        if (dock->frameGeometry() != previous) {
            previous = dock->frameGeometry();
            quiet.restart();
        } else if (quiet.elapsed() >= 200) return true;
        QThread::msleep(10);
    }
    return false;
}
}

bool verify_panel_drag(MainWindow *window) {
    auto *source = window->docks()->files;
    auto *target = window->docks()->sess;
    auto *body = source->widget();
    const QByteArray state = window->saveState();
    const QRect original = source->geometry();
    const QRect central = window->centralWidget()->geometry();
    auto *header = source->titleBarWidget();
    const QPoint upper = window->centralWidget()->mapToGlobal(
        QPoint(window->centralWidget()->width() / 2, window->centralWidget()->height() / 3));
    begin(source, upper);
    bool stable = !source->isFloating() && source->geometry() == original &&
        window->centralWidget()->geometry() == central && visible_preview(window);
    window->grab().save(QStringLiteral("native/target/panel-drag-preview.png"));
    cancel(header);
    mouse(header, QEvent::MouseButtonRelease, upper);
    bool cancelled = !source->isFloating() && source->geometry() == original && !visible_preview(window);
    window->restoreState(state);
    QApplication::processEvents();

    // A floating/native window can deliver release to the destination widget.
    // The gesture must still retire its preview and apply the same drop.
    bool foreign_release = true;
    for (bool tab_drop : {false, true}) for (bool floating_start : {false, true}) {
        source->setFloating(floating_start);
        source->show();
        QApplication::processEvents();
        auto *center_widget = window->centralWidget();
        const QPoint destination = tab_drop ? target->mapToGlobal(target->rect().center()) :
            center_widget->mapToGlobal(QPoint(center_widget->width() / 2, center_widget->height() / 3));
        begin(source, destination);
        const bool preview = visible_preview(window);
        mouse(center_widget, QEvent::MouseButtonRelease, destination);
        const bool placed = tab_drop ? window->tabifiedDockWidgets(target).contains(source) :
            window->dockWidgetArea(source) == Qt::TopDockWidgetArea;
        const bool delivered = preview && !source->isFloating() && source->widget() == body &&
            placed && !visible_preview(window);
        foreign_release &= delivered;
        std::printf("[cpp] C3 destination release tab-drop=%d start-floating=%d preview=%d floating=%d area=%d remaining-preview=%d: %s\n",
            tab_drop, floating_start, preview, source->isFloating(), int(window->dockWidgetArea(source)),
            visible_preview(window), delivered ? "PASS" : "FAIL");
        cancel(header); // Also retire the failed pre-fix gesture before restoring.
        window->restoreState(state);
        QApplication::processEvents();
    }
    std::printf("[cpp] C3 release delivered to destination: %s\n",
        foreign_release ? "PASS" : "FAIL");

    bool central_ok = true;
    for (bool top : {true, false}) {
        auto *center = window->centralWidget();
        const QPoint drop = center->mapToGlobal(QPoint(center->width() / 2,
            center->height() * (top ? 1 : 2) / 3));
        begin(source, drop);
        central_ok &= visible_preview(window);
        mouse(header, QEvent::MouseButtonRelease, drop);
        const QRect dock_rect = source->geometry(), center_rect = center->geometry();
        central_ok &= !source->isFloating() && source->widget() == body &&
            window->dockWidgetArea(source) == (top ? Qt::TopDockWidgetArea : Qt::BottomDockWidgetArea) &&
            (top ? dock_rect.bottom() < center_rect.top() : dock_rect.top() > center_rect.bottom()) &&
            !visible_preview(window);
        if (!top) window->grab().save(QStringLiteral("native/target/panel-drop-bottom.png"));
        window->restoreState(state);
        QApplication::processEvents();
    }

    bool split_ok = true;
    for (int side = 0; side < 4; ++side) {
        QPoint local = target->rect().center();
        if (side == 0) local.setY(target->height() / 6);
        if (side == 1) local.setY(target->height() * 5 / 6);
        if (side == 2) local.setX(target->width() / 6);
        if (side == 3) local.setX(target->width() * 5 / 6);
        const QPoint drop = target->mapToGlobal(local);
        begin(source, drop);
        split_ok &= visible_preview(window);
        mouse(header, QEvent::MouseButtonRelease, drop);
        const QRect a = source->geometry(), b = target->geometry();
        split_ok &= !source->isFloating() && !window->tabifiedDockWidgets(target).contains(source);
        split_ok &= side == 0 ? a.bottom() < b.top() : side == 1 ? a.top() > b.bottom() :
            side == 2 ? a.right() < b.left() : a.left() > b.right();
        window->restoreState(state);
        QApplication::processEvents();
    }

    const QPoint outside = window->mapToGlobal(QPoint(window->width() + 80, 120));
    begin(source, outside);
    const bool outside_preview = !visible_preview(window);
    mouse(header, QEvent::MouseButtonRelease, outside);
    const bool floated = source->isFloating() && source->widget() == body && source->width() <= 520;
    window->restoreState(state);
    QApplication::processEvents();
    const QPoint center = target->mapToGlobal(target->rect().center());
    begin(source, center);
    mouse(header, QEvent::MouseButtonRelease, center);
    const bool tabified = !source->isFloating() && window->tabifiedDockWidgets(target).contains(source);

    // 浮窗拖动：浮窗不再是主窗的子件，QWidget 的 MouseMove 会中断——跟手必须靠
    // 已装上的应用级过滤 + 直接把浮窗移到指针落点（否则浮窗「拖不动」，只在松手时跳一下）。
    bool float_ok = true;
    window->restoreState(state);
    QApplication::processEvents();
    auto *floating = window->docks()->files;
    floating->setFloating(true);
    floating->resize(420, 280);
    floating->show();
    // The preceding outside drop can leave the native frame at a screen edge.
    // Put this gesture inside the work area with room for its asserted 160x120
    // move, and settle native mapping before recording the starting geometry.
    if (auto *screen = QGuiApplication::screenAt(window->mapToGlobal(window->rect().center())))
        floating->move(screen->availableGeometry().topLeft() + QPoint(40, 40));
    const bool start_settled = settle_frame(floating);
    const QPoint f0 = floating->frameGeometry().topLeft();
    const QPoint grab = floating->titleBarWidget()->mapToGlobal(QPoint(8, 10));
    const QPoint slide(160, 120);
    mouse(floating->titleBarWidget(), QEvent::MouseButtonPress, grab);
    mouse(floating->titleBarWidget(), QEvent::MouseMove, grab + slide);
    const bool move_settled = settle_frame(floating);
    const QPoint f1 = floating->frameGeometry().topLeft();
    const bool float_shown = floating->isFloating();
    mouse(floating->titleBarWidget(), QEvent::MouseButtonRelease, grab + slide);
    // 松手落点由 `panel_drop_target` 决定：offscreen 平台会把浮窗外框夹回屏幕可用区，
    // 于是这个落点可能正压在左侧面板上 ⇒ 重停靠是对的。这里只断言「拖动不预先改形态」
    // 与「落点照常生效」，不断言松手后仍是浮窗（真实屏幕上是，合成环境里不是）。
    const bool float_shot =
        window->grab().save(QStringLiteral("native/target/panel-float-drag.png"));
    // 指针回到主窗内的面板上 ⇒ 预览出现 + 松手重新停靠。
    const QPoint redock_point = target->mapToGlobal(target->rect().center());
    mouse(floating->titleBarWidget(), QEvent::MouseButtonPress,
        floating->titleBarWidget()->mapToGlobal(QPoint(8, 10)));
    mouse(floating->titleBarWidget(), QEvent::MouseMove, redock_point);
    const bool float_preview = visible_preview(window);
    mouse(floating->titleBarWidget(), QEvent::MouseButtonRelease, redock_point);
    const bool float_redocked = !floating->isFloating();
    // 跟手判定比对位移（外框位置差）而非绝对值：窗口系统可能把浮窗夹进屏幕可用区，
    // 但「拖到哪、窗就挪到哪」这一条必须成立。
    const bool float_moved = (f1 - f0).x() > 100 && (f1 - f0).y() > 60;
    std::printf("[cpp] C3 float origin=%d,%d delta=%d,%d shown=%d preview=%d redocked=%d\n",
        f0.x(), f0.y(), (f1 - f0).x(), (f1 - f0).y(), float_shown, float_preview, float_redocked);
    float_ok &= start_settled && move_settled && float_shown && float_moved && float_shot &&
        float_preview && float_redocked;
    window->restoreState(state);
    QApplication::processEvents();

    const bool ok = stable && cancelled && foreign_release && central_ok && split_ok && outside_preview && floated &&
        tabified && float_ok;
    std::printf("[cpp] C3 panel drag stable=%d cancel=%d central=%d split-order=%d float=%d tabify=%d "
        "float-follow=%d: %s\n", stable, cancelled, central_ok, split_ok, floated, tabified, float_ok,
        ok ? "PASS" : "FAIL");
    return ok;
}
