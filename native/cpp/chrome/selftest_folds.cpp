// Qt owns the actual creation/menu/tree gestures and stale-dialog retirement.
#include "cpp/chrome/selftest_folds.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QDialog>
#include <QPushButton>
#include <QTimer>
#include <QTreeWidget>
#include <cstdio>

namespace {
void feed(uint32_t tab, const QByteArray &bytes) {
    term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
}
QTreeWidgetItem *item(QTreeWidget *tree, uint64_t id) {
    QTreeWidgetItemIterator it(tree);
    for (; *it; ++it) if ((*it)->data(0, Qt::UserRole).toULongLong() == id) return *it;
    return nullptr;
}
void click(QDialog *dialog, const char *name) { dialog->findChild<QPushButton *>(QLatin1String(name))->click(); }
void drag(PtyView *view, QPointF first, QPointF last) {
    QMouseEvent press(QEvent::MouseButtonPress, first, view->mapToGlobal(first), Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QMouseEvent move(QEvent::MouseMove, last, view->mapToGlobal(last), Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QMouseEvent release(QEvent::MouseButtonRelease, last, view->mapToGlobal(last), Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(view, &press); QApplication::sendEvent(view, &move); QApplication::sendEvent(view, &release);
}
}

bool selftest_ui::verify_folds(MainWindow &window, PtyView *view, const QString &png) {
    window.resize(1200, 900);
    if (!activate_window(window)) return false;
    const uint32_t tab = view->tab();
    term_set_reading(tab, false); term_clear(tab, 2);
    QByteArray bytes("\x1b]133;A\x07$ \x1b]133;B\x07" "echo FOLDS\r\n\x1b]133;C\x07");
    for (int row = 0; row < 12; ++row) bytes += QStringLiteral("FOLD_LINE_%1\r\n").arg(row, 2, 10, QLatin1Char('0')).toUtf8();
    bytes += "\x1b]133;D;0\x07\x1b]133;A\x07$ ";
    feed(tab, bytes); view->repaint();
    const Rect cell = term_caret_rect(tab);
    const QPointF origin = view->grid_pt(QPointF());
    const auto point = [&](int row, int col) { return QPointF(cell.x - 2 * cell.w - origin.x() + (col + 0.5) * cell.w,
        (row + 0.5) * cell.h - origin.y()); };
    auto *create = chrome::find_menu_action(window.menuBar(), QStringLiteral("view:fold-selection"));
    auto *manage = chrome::find_menu_action(window.menuBar(), QStringLiteral("view:fold-regions"));
    if (!create || !manage) return false;
    const auto edit = [&](const std::function<bool(QDialog *, QTreeWidget *)> &operation) {
        bool result = false; QObject guard;
        QTimer::singleShot(0, &guard, [&] {
            auto *dialog = window.findChild<QDialog *>(QStringLiteral("foldRegions"));
            if (!dialog) return;
            result = operation(dialog, dialog->findChild<QTreeWidget *>(QStringLiteral("foldTree")));
            if (dialog->isVisible()) dialog->reject();
        });
        manage->trigger(); return result;
    };
    drag(view, point(2, 1), point(7, 5)); window.refresh_chrome();
    bool ok = create->isEnabled(); create->trigger();
    uint64_t outer = 0;
    for (const auto &row : term_fold_rows(tab)) if (row.manual) outer = row.id;
    if (!outer) return false;
    ok &= edit([&](QDialog *dialog, QTreeWidget *tree) {
        auto *row = item(tree, outer); if (!row) return false;
        tree->setCurrentItem(row); click(dialog, "foldOpen");
        for (const auto &state : term_fold_rows(tab)) if (state.id == outer) return !state.folded;
        return false;
    });
    term_scroll_edge(tab, false);
    drag(view, point(3, 1), point(4, 5)); window.refresh_chrome(); create->trigger();
    uint64_t inner = 0, root = 0;
    for (const auto &row : term_fold_rows(tab)) { if (row.manual && row.id != outer) inner = row.id; if (!row.manual) root = row.id; }
    if (!inner || !root) return false;
    ok &= edit([&](QDialog *dialog, QTreeWidget *tree) {
        auto *child = item(tree, inner); if (!child || child->parent() != item(tree, outer)) return false;
        tree->setCurrentItem(item(tree, outer)); click(dialog, "foldClose");
        tree->setCurrentItem(item(tree, outer)); click(dialog, "foldOpen");
        bool retained = false;
        for (const auto &row : term_fold_rows(tab)) if (row.id == inner) retained = row.folded;
        tree->setCurrentItem(item(tree, outer)); click(dialog, "foldOpenRecursive");
        bool opened = true;
        for (const auto &row : term_fold_rows(tab)) if (row.manual) opened &= !row.folded;
        tree->setCurrentItem(item(tree, outer)); click(dialog, "foldCloseRecursive");
        bool closed = true;
        for (const auto &row : term_fold_rows(tab)) if (row.manual) closed &= row.folded;
        if (!png.isEmpty()) closed &= dialog->grab().save(png + QStringLiteral(".fold-tree.png"));
        tree->setCurrentItem(item(tree, outer)); click(dialog, "foldRemove");
        child = item(tree, inner);
        const bool reparented = child && child->parent() == item(tree, root);
        tree->setCurrentItem(item(tree, root));
        const bool protected_root = !dialog->findChild<QPushButton *>(QStringLiteral("foldRemove"))->isEnabled();
        tree->setCurrentItem(child); click(dialog, "foldJump");
        return retained && opened && closed && reparented && protected_root && !dialog->isVisible();
    });
    std::printf("[commands] nested fold pointer/menu/tree/open/recursive/remove/locate: %d\n", ok);

    MainWindow other;
    other.show();
    auto *retiring = other.active_view(); const uint32_t retired_tab = retiring->tab();
    term_disconnect_tab(retired_tab); term_clear(retired_tab, 2); feed(retired_tab, bytes); other.refresh_chrome();
    auto *other_manage = chrome::find_menu_action(other.menuBar(), QStringLiteral("view:fold-regions"));
    bool retired = false; QObject guard;
    QTimer::singleShot(0, &guard, [&] {
        auto *dialog = other.findChild<QDialog *>(QStringLiteral("foldRegions"));
        if (!dialog) return;
        other.close_current_tab(); click(dialog, "foldRefresh");
        retired = dialog->findChild<QTreeWidget *>(QStringLiteral("foldTree"))->topLevelItemCount() == 0 &&
            !dialog->findChild<QPushButton *>(QStringLiteral("foldJump"))->isEnabled();
        dialog->reject();
    });
    if (other_manage) other_manage->trigger();
    ok &= retired && term_fold_count(retired_tab) == 0;
    std::printf("[commands] fold dialog rejects retired source: %d\n", retired);
    window.activateWindow(); view->setFocus();
    return ok;
}
