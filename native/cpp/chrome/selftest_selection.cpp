#include "cpp/clipboard.h"
#include "cpp/chrome/selftest_clipboard.h"
// Cross-language selection risks: Qt pointer/modifier routing and active-pane policy.
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_selection_tools.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QDialog>
#include <QLabel>
#include <QPointer>
#include <QPushButton>
#include <QEventLoop>
#include <QKeyEvent>
#include <QFocusEvent>
#include <QMenuBar>
#include <QMouseEvent>
#include <QTimer>
#include <cstdio>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
// Only used for synthetic terminal selections, never for clipboard contents.
QString flat(const QString &s) { QString copy = s; return copy.replace('\n', '|'); }
void copy_and_dispatch(QAction *action) {
    // A menu gesture follows the preceding mouse/focus gesture in a separate
    // native event turn. In particular, finish any auto-copy notification first.
    QApplication::processEvents(QEventLoop::AllEvents, 10);
    action->trigger();
    // Each real menu gesture returns to the event loop. Drain its native clipboard
    // notifications before the next synthetic gesture; never retry the copy itself.
    QApplication::processEvents(QEventLoop::AllEvents, 10);
}
void key(PtyView *view, int code, const QString &text, Qt::KeyboardModifiers mods = Qt::NoModifier) {
    QKeyEvent event(QEvent::KeyPress, code, mods, text);
    QApplication::sendEvent(view, &event);
}
void drag(PtyView *view, QPointF start, QPointF end, Qt::KeyboardModifiers mods = Qt::NoModifier) {
    QMouseEvent press(QEvent::MouseButtonPress, start, view->mapToGlobal(start), Qt::LeftButton, Qt::LeftButton, mods);
    QMouseEvent move(QEvent::MouseMove, end, view->mapToGlobal(end), Qt::NoButton, Qt::LeftButton, mods);
    QMouseEvent release(QEvent::MouseButtonRelease, end, view->mapToGlobal(end), Qt::LeftButton, Qt::NoButton, mods);
    QApplication::sendEvent(view, &press);
    QApplication::sendEvent(view, &move);
    QApplication::sendEvent(view, &release);
}
// A press/release pair with no travel — what Shift+click does when extending a selection.
void click(PtyView *view, QPointF at, Qt::KeyboardModifiers mods = Qt::NoModifier) {
    QMouseEvent press(QEvent::MouseButtonPress, at, view->mapToGlobal(at), Qt::LeftButton, Qt::LeftButton, mods);
    QMouseEvent release(QEvent::MouseButtonRelease, at, view->mapToGlobal(at), Qt::LeftButton, Qt::NoButton, mods);
    QApplication::sendEvent(view, &press);
    QApplication::sendEvent(view, &release);
}
}

int qt_boot_selection(rust::Str png_out) {
    int argc = 1;
    char arg0[] = "native-selection";
    char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    MainWindow window;
    window.show();
    if (!selftest_ui::activate_window(window)) return 1;
    int verdict = 1;
    QTimer::singleShot(450, &window, [&] {
        bool ok = true;
        auto *view = window.active_view();
        const uint32_t tab = view->tab();
        const rust::String uid = term_uid(tab);
        term_disconnect_tab(tab);
        term_clear(tab, 2);
        const QByteArray bytes("0123456789\r\nABCDEFGHIJ\x1b[?1000h\x1b[?1006h");
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
        view->repaint();
        const Rect cell = term_caret_rect(tab);
        const QPointF origin = view->grid_pt(QPointF());
        const qreal left = cell.x - 10 * cell.w - origin.x();
        const auto at = [&](int col, int row) { return QPointF(left + (col + 0.5) * cell.w, (row + 0.5) * cell.h - origin.y()); };
        auto *column = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:toggle-stream-column"));
        auto *copy = chrome::find_menu_action(window.menuBar(), QStringLiteral("edit:copy"));
        if (!column || !copy) { app.exit(1); return; }
        window.refresh_chrome();
        ok &= column->isEnabled() && !column->isChecked();
        column->trigger();
        drag(view, at(2, 0), at(4, 1));
        ok &= term_column_selection(tab) && q(term_selected_text(tab)) == QStringLiteral("234\nCDE");
        copy_and_dispatch(copy);
        ok &= selftest_ui::clipboard_matches(QStringLiteral("234\nCDE"));
        std::printf("[selection] rectangle policy=%d selected-match=%d clipboard-match=%d\n", term_column_selection(tab), q(term_selected_text(tab)) == QStringLiteral("234\nCDE"), QApplication::clipboard()->text() == QStringLiteral("234\nCDE"));
        std::printf("[selection] mode/menu, mouse-grab override and rectangle copy: %d\n", ok);
        column->trigger();
        const QString stream = QStringLiteral("23456789\nABCDE");
        ok &= !term_column_selection(tab) && q(term_selected_text(tab)) == stream;
        copy_and_dispatch(copy);
        const bool current_copy = selftest_ui::clipboard_matches(stream);
        ok &= current_copy;
        std::printf("[selection] explicit copy reads converted current selection: %d\n", current_copy);

        // Exercise the menu itself under application mouse-grab: the next plain click must
        // extend locally, consume the one-shot intent and leave the pane's default mode alone.
        auto *target = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:select-to"));
        auto *target_column = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:select-column-to"));
        if (!target || !target_column) { app.exit(1); return; }
        const auto anchor_stream = [&] {
            term_deselect(tab);
            drag(view, at(2, 0), at(4, 0), Qt::ShiftModifier);
            window.refresh_chrome();
        };
        anchor_stream();
        target->trigger();
        click(view, at(6, 1));
        const bool menu_stream = q(term_selected_text(tab)) == QStringLiteral("23456789\nABCDEFG");
        const QString once = q(term_selected_text(tab));
        click(view, at(1, 0));
        const bool one_shot = q(term_selected_text(tab)) == once;
        anchor_stream();
        target_column->trigger();
        click(view, at(6, 1));
        const bool menu_column = !term_column_selection(tab) &&
            q(term_selected_text(tab)) == QStringLiteral("23456\nCDEFG");
        anchor_stream();
        const QString before_cancel = q(term_selected_text(tab));
        target->trigger();
        key(view, Qt::Key_Escape, {});
        click(view, at(6, 1));
        const bool cancel_kept = q(term_selected_text(tab)) == before_cancel;
        target_column->trigger();
        QFocusEvent focus_out(QEvent::FocusOut, Qt::OtherFocusReason);
        QApplication::sendEvent(view, &focus_out);
        click(view, at(6, 1));
        const bool focus_kept = q(term_selected_text(tab)) == before_cancel;
        target->trigger();
        term_deselect(tab);
        click(view, at(6, 1));
        const bool expired = q(term_selected_text(tab)).isEmpty();
        const bool menu_target = menu_stream && one_shot && menu_column && cancel_kept &&
            focus_kept && expired && term_uid(tab) == uid && !term_reading(tab);
        ok &= menu_target;
        std::printf("[selection] menu target stream=%d once=%d column=%d Esc=%d focus=%d expired=%d\n",
                    menu_stream, one_shot, menu_column, cancel_kept, focus_kept, expired);

        // B2D 接线：验证 Shift 选择到与交换锚点在 Qt 侧可达。选区**文本语义**由引擎层
        // selection_reference.rs 覆盖（那里直接驱动选区模型，不受像素坐标影响）；这里断言
        // 跨层接线：修饰键能到达 Rust 入口、菜单启用态随选区刷新、交换不改变已选文本。
        auto *swap = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:swap-anchor"));
        auto *select_to = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:select-to"));
        if (!swap || !select_to) { app.exit(1); return; }
        // 用本段已验证的 at() 建立一个真实拖拽选区（本段起点光标未动，度量仍然有效）。
        term_deselect(tab);
        drag(view, at(2, 0), at(4, 0), Qt::ShiftModifier);
        window.refresh_chrome();
        const QString anchored = q(term_selected_text(tab));
        const bool anchor_ok = !anchored.isEmpty();
        const bool enabled_with = swap->isEnabled() && select_to->isEnabled();
        ok &= anchor_ok && enabled_with;
        // 交换两端：覆盖文本逐字节不变，只有方向翻转。
        swap->trigger();
        const bool swap_kept = q(term_selected_text(tab)) == anchored;
        ok &= swap_kept;
        // 交换后再用 Shift 延伸：锚点未重置，选区仍非空。
        click(view, at(6, 1), Qt::ShiftModifier);
        const QString grown = q(term_selected_text(tab));
        const bool grew = !grown.isEmpty();
        ok &= grew;
        std::printf("[selection] select-to anchor=%d(%s) enabled_with=%d swap_kept=%d grew=%d(%s)\n",
                    anchor_ok, qPrintable(flat(anchored)), enabled_with,
                    swap_kept, grew, qPrintable(flat(grown)));
        std::printf("[selection] Shift select-to and swap-anchor route through Qt: %d\n", ok);
        if (!current_copy) std::printf("[selection] conversion policy=%d selected-match=%d clipboard-match=%d\n", term_column_selection(tab), q(term_selected_text(tab)) == stream, current_copy);

        drag(view, at(2, 0), at(4, 1), Qt::AltModifier);
        ok &= !term_column_selection(tab) && q(term_selected_text(tab)) == QStringLiteral("234\nCDE");
        copy_and_dispatch(copy);
        ok &= selftest_ui::clipboard_matches(QStringLiteral("234\nCDE"));

        // B2E: this layer owns pointer modifiers, menu dispatch, focus and the Qt clipboard.
        // Source geometry, budgets and pruning are asserted by the engine suite.
        term_deselect(tab);
        drag(view, at(0, 0), at(1, 0), Qt::ShiftModifier);
        const uint64_t first_id = term_selection_status(tab).active_id;
        drag(view, at(5, 1), at(7, 1), Qt::ControlModifier);
        drag(view, at(2, 0), at(4, 1), Qt::ControlModifier | Qt::AltModifier);
        auto *next = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:activate-next"));
        auto *previous = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:activate-previous"));
        auto *remove = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:delete"));
        auto *multi = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:multiple"));
        if (!next || !previous || !remove || !multi) { app.exit(1); return; }
        window.refresh_chrome();
        const QString joined = QStringLiteral("01\n234\nCDE\nFGH");
        const bool appended = term_selection_status(tab).count == 3 &&
            q(term_selected_text(tab)) == joined && next->isEnabled() && previous->isEnabled();
        copy_and_dispatch(copy);
        const bool all_copy = selftest_ui::clipboard_matches(joined);
        next->trigger();
        const bool cycled = term_selection_status(tab).active_id == first_id;
        click(view, at(2, 0), Qt::ShiftModifier);
        const bool active_extended = q(term_selected_text(tab)) == QStringLiteral("012\n234\nCDE\nFGH");
        previous->trigger(); remove->trigger();
        const bool removed = term_selection_status(tab).count == 2 &&
            q(term_selected_text(tab)) == QStringLiteral("012\nFGH");
        multi->trigger();
        drag(view, at(8, 0), at(9, 0));
        const bool policy_added = multi->isChecked() && term_multi_selection(tab) &&
            term_selection_status(tab).count == 3;
        multi->trigger();
        // Reading Tab must reach the pane rather than QWidget's default focus traversal.
        term_set_reading(tab, true); view->sync_reading_mode();
        const uint64_t before_tab = term_selection_status(tab).active_id;
        key(view, Qt::Key_Tab, {});
        const bool tab_cycle = term_selection_status(tab).active_id != before_tab && view->hasFocus();
        key(view, Qt::Key_Backtab, {}, Qt::ShiftModifier);
        const bool backtab = term_selection_status(tab).active_id == before_tab;
        key(view, Qt::Key_Escape, {});
        const bool multi_clear = term_selection_status(tab).count == 0 && !next->isEnabled();
        term_set_reading(tab, false); view->sync_reading_mode();
        const bool multi_ok = appended && all_copy && cycled && active_extended && removed &&
            policy_added && tab_cycle && backtab && multi_clear && term_uid(tab) == uid;
        ok &= multi_ok;
        std::printf("[selection] multiple append=%d copy=%d cycle=%d extend=%d remove=%d policy=%d Tab=%d Backtab=%d clear=%d\n",
            appended, all_copy, cycled, active_extended, removed, policy_added, tab_cycle, backtab, multi_clear);

        ok &= selftest_ui::verify_selection_tools(window, view, at);
        term_clear(tab, 2);
        const QByteArray unicode = QStringLiteral("A中e\u0301Z\r\nAbcdZ").toUtf8();
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(unicode.data()), size_t(unicode.size())));
        view->repaint();
        drag(view, at(2, 0), at(3, 1), Qt::AltModifier);
        copy_and_dispatch(copy);
        const QString expected_unicode = QStringLiteral("中e\u0301\ncd");
        const QString unicode_selection = q(term_selected_text(tab));
        const bool unicode_copy = selftest_ui::clipboard_matches(expected_unicode);
        if (!unicode_copy) std::printf("[selection] Unicode copy selected-match=%d clipboard-match=%d\n",
            unicode_selection == expected_unicode, unicode_copy);
        ok &= unicode_copy;
        std::printf("[selection] wide/combining edge copy through Qt: %d\n", ok);
        term_clear(tab, 2);
        term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
        view->repaint();
        term_set_reading(tab, true);
        key(view, Qt::Key_G, QStringLiteral("g")); key(view, Qt::Key_G, QStringLiteral("g"));
        key(view, Qt::Key_V, QString(QChar(0x16)), Qt::ControlModifier);
        key(view, Qt::Key_Right, {}); key(view, Qt::Key_Right, {}); key(view, Qt::Key_Down, {});
        ok &= term_reading(tab) && q(term_selected_text(tab)) == QStringLiteral("012\nABC");
        // 阅读 `o` 交换视觉选区两端：矩形没有方向可翻，覆盖文本必须原样保留。
        const QString rect = q(term_selected_text(tab));
        key(view, Qt::Key_O, QStringLiteral("o"));
        const bool o_kept = term_reading(tab) && q(term_selected_text(tab)) == rect;
        ok &= o_kept;
        key(view, Qt::Key_Escape, {});
        const bool escaped = q(term_selected_text(tab)).isEmpty() && term_uid(tab) == uid;
        ok &= escaped;
        std::printf("[selection] reading rect=(%s) o_kept=%d escaped=%d\n",
                    qPrintable(flat(rect)), o_kept, escaped);
        std::printf("[selection] Alt drag and reading Ctrl+V stay pane-local: %d\n", ok);
        // Windows grabs process clipboard events. Finish copy assertions first,
        // then reproduce the two selections solely for painting evidence.
        if (!png_out.empty()) {
            term_set_reading(tab, false); view->sync_reading_mode();
            const QString path = QString::fromUtf8(png_out.data(), int(png_out.size()));
            const auto capture = [&](const QByteArray &data, int cursor_col, int last_col, const QString &suffix) {
                term_clear(tab, 2);
                term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(data.data()), size_t(data.size())));
                view->repaint();
                const Rect glyph = term_caret_rect(tab);
                const QPointF pad = view->grid_pt(QPointF());
                const qreal x = glyph.x - cursor_col * glyph.w - pad.x();
                const auto point = [&](int col, int row) { return QPointF(x + (col + 0.5) * glyph.w, (row + 0.5) * glyph.h - pad.y()); };
                drag(view, point(2, 0), point(last_col, 1), Qt::AltModifier);
                view->repaint();
                return window.grab().save(path + suffix);
            };
            ok &= capture(bytes, 10, 4, QString());
            ok &= capture(unicode, 5, 3, QStringLiteral(".unicode.png"));
            term_clear(tab, 2);
            term_feed_inject(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), size_t(bytes.size())));
            view->repaint();
            drag(view, at(0, 0), at(1, 0), Qt::ShiftModifier);
            drag(view, at(5, 1), at(7, 1), Qt::ControlModifier);
            drag(view, at(2, 0), at(4, 1), Qt::ControlModifier | Qt::AltModifier);
            view->repaint();
            ok &= term_selection_status(tab).count == 3 && window.grab().save(path + QStringLiteral(".multi.png"));
        }
        term_set_column_selection(tab, true);
        term_set_multi_selection(tab, true);
        window.split_active(Qt::Horizontal);
        QApplication::processEvents();
        auto *other = window.active_view();
        ok &= other != view && !term_column_selection(other->tab()) &&
            !term_multi_selection(other->tab()) && term_selection_status(other->tab()).count == 0;
        view->setFocus(); window.refresh_chrome(); ok &= column->isChecked();
        other->setFocus(); window.refresh_chrome(); ok &= !column->isChecked();
        // B2F owns modeless snapshot/clipboard/owner lifetime here, not engine geometry.
        view->setFocus(); term_select_all(tab); window.refresh_chrome();
        auto *preview_action = chrome::find_menu_action(window.menuBar(), QStringLiteral("select:preview"));
        if (!preview_action) { app.exit(1); return; }
        preview_action->trigger();
        // Complete native dialog activation before the next synthetic click.
        // The standalone Qt probe reproduces an empty OLE clipboard if these
        // two real user gestures are compressed into one event callback.
        QApplication::processEvents(QEventLoop::AllEvents, 10);
        QPointer<QDialog> preview = view->findChild<QDialog *>(QStringLiteral("terminalPreview"));
        if (!preview) { app.exit(1); return; }
        auto *preview_text = preview->findChild<QLabel *>(QStringLiteral("previewText"));
        auto *preview_copy = preview->findChild<QPushButton *>(QStringLiteral("previewCopy"));
        auto *preview_refresh = preview->findChild<QPushButton *>(QStringLiteral("previewRefresh"));
        const QString preview_expected = QStringLiteral("0123456789\nABCDEFGHIJ");
        bool preview_ok = true;
        const auto check_preview = [&](const char *stage, bool result) {
            preview_ok &= result;
            std::printf("[selection] preview %s=%d\n", stage, result);
        };
        check_preview("initial", !preview->isModal() && preview_text->text().trimmed() == preview_expected);
        preview_copy->click();
        QApplication::processEvents(QEventLoop::AllEvents, 10);
        check_preview("copied", selftest_ui::clipboard_matches(preview_expected, true));
        term_deselect(tab); // Selection edits do not mutate an already opened snapshot.
        check_preview("snapshot kept", preview_text->text().trimmed() == preview_expected);
        preview_refresh->click();
        check_preview("deselected", preview_text->text().isEmpty() && !preview_copy->isEnabled());
        term_select_all(tab); preview_refresh->click();
        view->setFocus(); window.refresh_chrome(); preview_action->trigger();
        check_preview("reused", view->findChildren<QDialog *>(QStringLiteral("terminalPreview")).size() == 1);
        other->setFocus();
        const QByteArray other_bytes("OTHER PANE");
        term_feed_inject(other->tab(), rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(other_bytes.data()), size_t(other_bytes.size())));
        term_select_all(other->tab());
        preview_refresh->click();
        check_preview("owner", preview_text->text().trimmed() == preview_expected);
        QApplication::processEvents(QEventLoop::AllEvents, 10);
        check_preview("guard setup", host_clipboard::copy_text(QStringLiteral("PREVIEW_LOCK_GUARD")));
        // The external clipboard setup and the subsequent lock/copy gesture
        // also occupy separate event turns; finish the OLE update before locking.
        QApplication::processEvents(QEventLoop::AllEvents, 10);
        window.setProperty("screenLocked", true); preview_copy->click();
        window.setProperty("screenLocked", false);
        check_preview("locked", preview_text->text().isEmpty() && selftest_ui::clipboard_matches(QStringLiteral("PREVIEW_LOCK_GUARD")));
        preview_refresh->click();
        if (!png_out.empty()) preview_ok &= preview->grab().save(
            QString::fromUtf8(png_out.data(), int(png_out.size())) + QStringLiteral(".preview.png"));
        window.close_current_tab();
        QApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        check_preview("retired", preview.isNull());
        ok &= preview_ok;
        std::printf("[selection] preview snapshot/copy/refresh/pane isolation/lock/retirement: %d\n", preview_ok);
        ok &= !term_set_column_selection(tab, true) && !term_column_selection(tab);
        std::printf("[selection] split focus and retired pane policy: %d\n", ok);
        QApplication::clipboard()->clear();
        verdict = ok ? 0 : 1;
        app.exit(verdict);
    });
    QTimer::singleShot(15000, &app, [&] { app.exit(2); });
    app.exec();
    return verdict;
}
