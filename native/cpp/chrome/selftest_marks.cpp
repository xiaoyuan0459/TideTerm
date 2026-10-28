// Qt owns real gestures, editor binding, visible paint and default-profile initialization.
#include "cpp/chrome/selftest_marks.h"
#include "cpp/i18n.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTimer>
#include <QTreeWidget>
#include <cstdio>
#include <QStatusBar>

namespace {
void feed(uint32_t tab, const QByteArray &value) {
    term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(value.data()), size_t(value.size())));
}
void click(QDialog *d, const char *name) { d->findChild<QPushButton *>(QLatin1String(name))->click(); }
QTreeWidgetItem *row(QTreeWidget *tree, uint64_t id) {
    for (int i = 0; i < tree->topLevelItemCount(); ++i)
        if (tree->topLevelItem(i)->data(0, Qt::UserRole).toULongLong() == id) return tree->topLevelItem(i);
    return nullptr;
}
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
}

bool selftest_ui::verify_marks(MainWindow &window, PtyView *view, const QString &png) {
    window.resize(1200, 900); if (!activate_window(window)) return false;
    const uint32_t tab = view->tab(); const auto uid = term_uid(tab);
    term_set_reading(tab, false); term_clear(tab, 2); term_mark_clear(tab);
    const QByteArray bytes("\x1b]133;A\007$ \x1b]133;B\007echo MARKS\r\n\x1b]133;C\007"
        "ERROR WARN ERROR\r\nINFO 中é WARN\r\nERROR WARN\r\n\x1b]133;D;0\007\x1b]133;A\007$ ");
    feed(tab, bytes); view->repaint();
    const Rect cell = term_caret_rect(tab); const QPointF origin = view->grid_pt(QPointF());
    const auto point = [&](int line, int col) { return QPointF(cell.x - 2 * cell.w - origin.x() + (col + 0.5) * cell.w,
        (line + 0.5) * cell.h - origin.y()); };
    const QPointF first = point(1, 0), last = point(1, 4);
    QMouseEvent press(QEvent::MouseButtonPress, first, view->mapToGlobal(first), Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QMouseEvent move(QEvent::MouseMove, last, view->mapToGlobal(last), Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QMouseEvent release(QEvent::MouseButtonRelease, last, view->mapToGlobal(last), Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(view, &press); QApplication::sendEvent(view, &move); QApplication::sendEvent(view, &release);
    window.refresh_chrome();
    auto *next_selection = chrome::find_menu_action(window.menuBar(), QStringLiteral("search:find-next-selection"));
    auto *previous_selection = chrome::find_menu_action(window.menuBar(), QStringLiteral("search:find-previous-selection"));
    auto *manage = chrome::find_menu_action(window.menuBar(), QStringLiteral("search:highlight"));
    bool ok = next_selection && previous_selection && manage && next_selection->isEnabled() && previous_selection->isEnabled();
    if (!ok) return false;
    next_selection->trigger();
    ok &= q(term_search_query(tab)) == QStringLiteral("ERROR") && term_search_active_rect(tab).x > cell.w;
    previous_selection->trigger(); ok &= term_uid(tab) == uid && !term_reading(tab);
    term_search_set(tab, rust::Str(""), 0);
    const QPoint sample = ((point(1, 12) + QPointF(cell.w * 0.4, cell.h * 0.4)) * view->devicePixelRatioF()).toPoint();
    const QColor before = view->grab().toImage().pixelColor(sample);
    bool editor = false; QObject guard;
    QTimer::singleShot(0, &guard, [&] {
        auto *dialog = window.findChild<QDialog *>(QStringLiteral("textMarks")); if (!dialog) return;
        auto *tree = dialog->findChild<QTreeWidget *>(QStringLiteral("markRules"));
        auto *pattern = dialog->findChild<QLineEdit *>(QStringLiteral("markPattern"));
        auto *regex = dialog->findChild<QCheckBox *>(QStringLiteral("markRegex"));
        auto *enabled = dialog->findChild<QCheckBox *>(QStringLiteral("markEnabled"));
        auto *color = dialog->findChild<QComboBox *>(QStringLiteral("markColor"));
        auto *save_on_close = dialog->findChild<QCheckBox *>(QStringLiteral("markSaveOnClose"));
        bool pass = pattern->text() == QStringLiteral("ERROR") && save_on_close;
        if (!save_on_close) { dialog->reject(); return; }
        save_on_close->setChecked(true);
        pass &= sess_active_config(tab).options.mark_save_on_close;
        save_on_close->setChecked(false);
        pass &= !sess_active_config(tab).options.mark_save_on_close;
        click(dialog, "markQuick2"); auto rules = term_mark_rows(tab);
        if (rules.size() != 1) { dialog->reject(); return; }
        const uint64_t literal = rules[0].id;
        const bool painted = view->grab().toImage().pixelColor(sample) != before;
        click(dialog, "markNew"); pattern->setText(QStringLiteral("ERROR|WARN")); regex->setChecked(true); color->setCurrentIndex(1);
        click(dialog, "markAdd"); rules = term_mark_rows(tab); uint64_t expression = 0;
        for (const auto &r : rules) if (r.flags & 2) expression = r.id;
        pass &= rules.size() == 2 && expression != 0;
        if (!expression) { dialog->reject(); return; }
        pattern->setText(QStringLiteral("WARN|ERROR"));
        save_on_close->setChecked(true);
        const bool draft_preserved = pattern->text() == QStringLiteral("WARN|ERROR") && tree->currentItem()
            && tree->currentItem()->data(0, Qt::UserRole).toULongLong() == expression;
        save_on_close->setChecked(false);
        std::printf("[commands] mark save mode preserves pending edit and selected rule: %d\n", draft_preserved);
        pass &= draft_preserved;
        QKeyEvent submit(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QStringLiteral("\r"));
        QApplication::sendEvent(dialog, &submit);
        bool edited = false;
        for (const auto &r : term_mark_rows(tab)) if (r.id == expression) edited = q(r.pattern) == QStringLiteral("WARN|ERROR");
        pass &= edited && term_mark_rows(tab).size() == 2;
        const QString before_error = dialog->findChild<QLabel *>(QStringLiteral("markStatus"))->text();
        pattern->setText(QStringLiteral("("));
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QStringLiteral("\r"));
        QApplication::sendEvent(dialog, &enter);
        bool retained = false; for (const auto &r : term_mark_rows(tab)) if (r.id == expression) retained = q(r.pattern) == QStringLiteral("WARN|ERROR");
        pass &= retained && dialog->findChild<QLabel *>(QStringLiteral("markStatus"))->text() != before_error;
        pattern->setText(QStringLiteral("ERROR|WARN")); enabled->setChecked(false); click(dialog, "markApply");
        for (const auto &r : term_mark_rows(tab)) if (r.id == expression) pass &= !r.enabled;
        enabled->setChecked(true); click(dialog, "markApply"); click(dialog, "markUp");
        pass &= term_mark_rows(tab)[0].id == expression;
        click(dialog, "markSave");
        const uint32_t future = term_new_tab(); term_disconnect_tab(future);
        pass &= term_mark_rows(future).size() == 2; term_close_tab(future);
        tree->setCurrentItem(row(tree, literal)); click(dialog, "markRemove");
        const auto remaining = term_mark_rows(tab); pass &= remaining.size() == 1 && remaining[0].id == expression;
        click(dialog, "markClear"); pass &= term_mark_rows(tab).empty() && term_mark_rects(tab).empty();
        click(dialog, "markLoad"); pass &= term_mark_rows(tab).size() == 2;
        if (!png.isEmpty()) pass &= dialog->grab().save(png + QStringLiteral(".text-marks.png")) && view->grab().save(png + QStringLiteral(".marked.png"));
        editor = pass && painted; dialog->accept();
    });
    manage->trigger(); ok &= editor;
    std::printf("[commands] marks pointer/selection-search/editor/invalid/priority/defaults/paint: %d\n", ok);
    MainWindow other; other.show(); auto *retiring = other.active_view();
    const uint32_t retired_tab = retiring->tab(); term_disconnect_tab(retired_tab); other.refresh_chrome();
    bool retired = false; QObject retired_guard;
    QTimer::singleShot(0, &retired_guard, [&] {
        auto *dialog = other.findChild<QDialog *>(QStringLiteral("textMarks")); if (!dialog) return;
        other.close_current_tab(); click(dialog, "markRefresh");
        retired = dialog->findChild<QTreeWidget *>(QStringLiteral("markRules"))->topLevelItemCount() == 0 &&
            !dialog->findChild<QPushButton *>(QStringLiteral("markAdd"))->isEnabled() &&
            !dialog->findChild<QPushButton *>(QStringLiteral("markSave"))->isEnabled();
        dialog->reject();
    });
    if (auto *action = chrome::find_menu_action(other.menuBar(), QStringLiteral("search:highlight"))) action->trigger();
    ok &= retired && term_mark_rows(retired_tab).empty();
    std::printf("[commands] mark dialog rejects retired pane/default writes: %d\n", retired);
    MainWindow closing; closing.show();
    auto *source = closing.active_view(); const uint32_t source_tab = source->tab();
    term_disconnect_tab(source_tab);
    auto *close_manage = chrome::find_menu_action(closing.menuBar(), QStringLiteral("search:highlight"));
    bool configured = false; QObject close_guard;
    QTimer::singleShot(0, &close_guard, [&] {
        auto *dialog = closing.findChild<QDialog *>(QStringLiteral("textMarks")); if (!dialog) return;
        auto *mode = dialog->findChild<QCheckBox *>(QStringLiteral("markSaveOnClose"));
        auto *pattern = dialog->findChild<QLineEdit *>(QStringLiteral("markPattern"));
        if (!mode || !pattern) { dialog->reject(); return; }
        mode->setChecked(true); click(dialog, "markNew");
        pattern->setText(QStringLiteral("CLOSE_RULE")); click(dialog, "markAdd");
        configured = term_mark_rows(source_tab).size() == 3;
        dialog->accept();
    });
    if (close_manage) close_manage->trigger();
    if (configured) closing.close_current_tab();
    const uint32_t loaded = term_new_tab(); term_disconnect_tab(loaded);
    bool saved_close = configured && closing.page_count() == 0;
    bool found = false; for (const auto &rule : term_mark_rows(loaded)) found |= q(rule.pattern) == QStringLiteral("CLOSE_RULE");
    saved_close &= found; term_close_tab(loaded);
    // An older dirty pane is retained when another pane already changed defaults.
    term_mark_set_save_on_close(tab, true);
    term_mark_put(tab, 0, "UNSAVED_CONFLICT", 1, 0, true);
    const int page_count = window.page_count();
    window.close_tab(window.current_page());
    const auto *close_note = window.findChild<QLabel *>(QStringLiteral("statusMsg"));
    const bool conflict_retained = window.page_count() == page_count && !view->is_closed()
        && close_note && close_note->text().startsWith(i18n::tr("未关闭：标记尚未保存。"));
    term_mark_set_save_on_close(tab, false);
    // Restore the earlier default rules for the independent remaining commands cases.
    if (!view->is_closed()) {
        for (const auto &rule : term_mark_rows(tab)) if (q(rule.pattern) == QStringLiteral("UNSAVED_CONFLICT")) term_mark_remove(tab, rule.id);
        term_mark_defaults(tab, true);
    }
    std::printf("[commands] marks close persisted=%d conflicting-close-retained=%d\n", saved_close, conflict_retained);
    ok &= saved_close && conflict_retained;
    window.activateWindow(); view->setFocus(); return ok;
}
