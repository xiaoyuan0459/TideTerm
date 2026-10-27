// Actual Qt/PTY flow: full session splits, caret popup and draft-only acceptance.
#include "cpp/chrome/selftest_terminal_suggestions.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QLabel>
#include <QPointer>
#include <QScreen>
#include <QTabWidget>
#include <QTabBar>
#include <QTimer>
#include <QTreeWidget>
#include <cstdio>
#include <functional>

namespace {
bool await(const std::function<bool()> &predicate, int budget = 6000) {
    QElapsedTimer clock; clock.start();
    while (clock.elapsed() < budget) {
        if (predicate()) return true;
        QEventLoop turn; QTimer::singleShot(20, &turn, &QEventLoop::quit); turn.exec();
    }
    return predicate();
}
void key(PtyView *view, int code, Qt::KeyboardModifiers mods = Qt::NoModifier, bool repeat = false) {
    const QString text = code == Qt::Key_C ? QStringLiteral("c") : QString();
    QKeyEvent press(QEvent::KeyPress, code, mods, text, repeat);
    QApplication::sendEvent(view, &press);
    if (!repeat) { QKeyEvent release(QEvent::KeyRelease, code, mods); QApplication::sendEvent(view, &release); }
}
bool prompt(PtyView *view) {
    const auto snapshot = terminal_completion_query(view->tab());
    return snapshot.batch.context.ready && snapshot.query.empty();
}
bool layout(MainWindow &window) {
    bool ok = true;
    for (auto *group : window.workspace()->groups()) {
        auto *page = group->currentWidget();
        const auto panes = page ? MainWindow::views(page) : std::vector<PtyView *>{};
        auto *view = panes.empty() ? nullptr : panes.front();
        SessionBanner *banner = nullptr;
        if (page) for (QObject *child : page->children()) if (auto *found = dynamic_cast<SessionBanner *>(child)) banner = found;
        auto *footer = page ? page->findChild<QLabel *>(QStringLiteral("sessionFooter"), Qt::FindDirectChildrenOnly) : nullptr;
        const bool complete = page && view && banner && footer && banner->isVisible() && footer->isVisible()
            && !footer->text().isEmpty() && group->tabBar()->isVisible();
        ok &= complete;
        if (!complete) continue;
        for (QWidget *widget = view; widget != window.workspace() && widget->parentWidget(); widget = widget->parentWidget())
            ok &= widget->parentWidget()->rect().contains(widget->geometry());
        ok &= page->rect().contains(banner->geometry()) && page->rect().contains(footer->geometry());
        const auto stats = term_stats(view->tab());
        // 同上：2 列下限时栏可能放不下（窗格 < 8 列），属已知边界。
        ok &= (stats.cols + stats.gutter_cells) * stats.cell_w <=
                  view->grid_w() * view->devicePixelRatioF() ||
            stats.cols <= 2;
        ok &= stats.rows * stats.cell_h <= view->grid_h() * view->devicePixelRatioF();
    }
    return ok;
}
}

bool verify_terminal_suggestions(MainWindow &window) {
    while (window.page_count() > 0) window.close_tab(0);
    const QSize available = window.screen()->availableGeometry().size() - QSize(40, 40);
    window.resize(qMin(1300, available.width()), qMin(860, available.height()));
    window.add_tab(); window.add_tab();
    if (!selftest_ui::activate_window(window)) return false;
    const uint32_t retained = window.page_id(1);
    const auto uid = term_uid(retained);
    auto *horizontal = chrome::find_menu_action(window.menuBar(), QStringLiteral("window:split-view-horizontal"));
    auto *vertical = chrome::find_menu_action(window.menuBar(), QStringLiteral("window:split-view-vertical"));
    if (!horizontal || !vertical) return false;
    // The public Focus Mode frees space on small screens. First prove that an
    // undersized area refuses to create a cropped session or orphan process.
    window.workspace()->setMaximumWidth(320);
    if (!await([&] { return window.workspace()->groups().front()->width() <= 320; })) return false;
    std::printf("[split] constrained area width=%d\n", window.workspace()->groups().front()->width());
    horizontal->trigger();
    bool ok = window.workspace()->groups().size() == 1 && window.page_count() == 2 && term_tab_count() == 2;
    std::printf("[split] constrained groups=%d pages=%d action-enabled=%d\n", int(window.workspace()->groups().size()), window.page_count(), horizontal->isEnabled());
    window.workspace()->setMaximumWidth(QWIDGETSIZE_MAX);
    window.resize(qMin(1300, available.width()), qMin(860, available.height()));
    auto *focus = chrome::find_menu_action(window.menuBar(), QStringLiteral("mode:focus"));
    if (!focus) return false;
    focus->trigger();
    if (!await([&] { return window.workspace()->groups().front()->width() >= 485; })) {
        std::printf("[split] focus enabled=%d active=%d window=%d workspace=%d group=%d groups=%d\n", focus->isEnabled(),
            window.property("focusMode").toBool(), window.width(), window.workspace()->width(), window.workspace()->groups().front()->width(), int(window.workspace()->groups().size()));
        return false;
    }
    std::printf("[split] usable area width=%d\n", window.workspace()->groups().front()->width());
    horizontal->trigger();
    ok &= await([&] {
        window.refresh_chrome();
        return window.workspace()->groups().size() == 2 && window.page_count() == 3
            && window.workspace()->groups().last()->height() >= 285 && layout(window);
    });
    std::printf("[split] horizontal groups=%d pages=%d new-height=%d ready=%d\n", int(window.workspace()->groups().size()),
        window.page_count(), window.workspace()->groups().last()->height(), ok);
    vertical->trigger();
    ok &= await([&] { window.refresh_chrome(); return window.workspace()->groups().size() == 3 && layout(window); });
    ok &= window.page_count() == 4 && term_uid(retained) == uid;
    std::printf("[split] vertical groups=%d pages=%d layout=%d\n", int(window.workspace()->groups().size()), window.page_count(), layout(window));
    std::printf("[split] default actions preserve existing tabs and split complete session areas: %s\n", ok ? "PASS" : "FAIL");
    if (!ok) return false;
    auto *view = window.active_view();
    QPointer<PtyView> observed(view);
    const auto diagnostic = [&](const char *phase) {
        if (!observed) { std::printf("[completion] %s no active view\n", phase); return; }
        const auto state = terminal_completion_query(observed->tab());
        const auto screen = term_screen_text(observed->tab());
        std::printf("[completion] %s ready=%d query=[%.*s] reading=%d screen=[%.*s]\n", phase,
            state.batch.context.ready, int(state.query.size()), state.query.data(), term_reading(observed->tab()), int(screen.size()), screen.data());
    };
    const char *phase = "initial-prompt";
    struct FailureTrace {
        std::function<void()> diagnostic;
        bool succeeded = false;
        ~FailureTrace() { if (!succeeded) diagnostic(); }
    } trace{[&] { diagnostic(phase); }};
    if (!view || !await([&] { return prompt(view); })) return false;
    auto *popup = view->findChild<QTreeWidget *>(QStringLiteral("terminalSuggestions"));
    if (!popup) return false;
    const auto pane_contains_popup = [&] {
        return QRect(view->mapToGlobal(QPoint()), view->size()).contains(popup->geometry());
    };
    phase = "vocabulary-rail";
#ifdef Q_OS_WIN
    const QString vocabulary_query = QStringLiteral("Get-Ch");
    const QString vocabulary_command = QStringLiteral("Get-ChildItem");
#else
    const QString vocabulary_query = QStringLiteral("pw");
    const QString vocabulary_command = QStringLiteral("pwd");
#endif
    const auto vocabulary_revision = history_revision(view->tab());
    view->inject_ime(vocabulary_query, QString());
    if (!await([&] { return popup->isVisible() && popup->topLevelItemCount() > 0; })) return false;
    const bool vocabulary_rail = popup->topLevelItem(0)->text(0) == vocabulary_command
        && popup->topLevelItem(0)->text(1) == QLatin1String("c") && pane_contains_popup()
        && popup->topLevelItem(0)->toolTip(0).contains(QStringLiteral("https://"))
        && popup->font().pixelSize() == int(term_font_px());
    std::printf("[completion] rail text=[%s] badge=[%s] tooltip=%d font=%d expected-font=%u bounds=%d view=%dx%d@%d,%d popup=%dx%d@%d,%d\n",
        popup->topLevelItem(0)->text(0).toUtf8().constData(), popup->topLevelItem(0)->text(1).toUtf8().constData(),
        popup->topLevelItem(0)->toolTip(0).contains(QStringLiteral("https://")), popup->font().pixelSize(), term_font_px(), pane_contains_popup(),
        view->width(), view->height(), view->mapToGlobal(QPoint()).x(), view->mapToGlobal(QPoint()).y(),
        popup->width(), popup->height(), popup->x(), popup->y());
    window.grab().save(QStringLiteral("native/target/compact-popup-vocabulary.png"));
    key(view, Qt::Key_Tab);
    const bool vocabulary_fill = await([&] { return terminal_completion_query(view->tab()).query == vocabulary_command.toStdString(); })
        && history_revision(view->tab()) == vocabulary_revision;
    std::printf("[completion] compact vocabulary c rail / terminal font / pane bounds / draft only: %d/%d\n", vocabulary_rail, vocabulary_fill);
    if (!vocabulary_rail || !vocabulary_fill) return false;
    key(view, Qt::Key_C, Qt::ControlModifier);
    if (!await([&] { return prompt(view); })) return false;
    for (const QString &command : {QStringLiteral("echo __TIDE_POPUP_ALPHA__"), QStringLiteral("echo __TIDE_POPUP_BETA__")}) {
        view->inject_ime(command, QString());
        if (!await([&] { const auto s = terminal_completion_query(view->tab()); return QString::fromUtf8(s.query.data(), int(s.query.size())) == command; })) { diagnostic("history-echo"); return false; }
        key(view, Qt::Key_Return);
        if (!await([&] { return prompt(view); })) { diagnostic("history-next-prompt"); return false; }
    }
    phase = "empty-draft-history";
    view->inject_ime(QStringLiteral("x"),QString());
    if (!await([&] { return terminal_completion_query(view->tab()).query=="x"; })) return false;
    key(view,Qt::Key_Backspace);
    if (!await([&] { return prompt(view); })) { diagnostic("cancel-or-submit-prompt"); return false; }
    QEventLoop empty_turn; QTimer::singleShot(180,&empty_turn,&QEventLoop::quit); empty_turn.exec();
    const bool quiet_empty=!popup->isVisible();
    key(view,Qt::Key_Space,Qt::ControlModifier);
    const bool history_only=await([&] { return popup->isVisible() && popup->topLevelItemCount()==2; })
        && popup->topLevelItem(0)->text(0).contains(QStringLiteral("__TIDE_POPUP_"))
        && popup->topLevelItem(0)->text(1) == QLatin1String("h")
        && !popup->topLevelItem(0)->toolTip(1).isEmpty();
    key(view,Qt::Key_Escape);
    std::printf("[completion] automatic empty stays quiet / explicit empty browses this terminal history: %d/%d\n",quiet_empty,history_only);
    if (!quiet_empty || !history_only) return false;
    phase = "first-accept";
    const QString prefix = QStringLiteral("echo __TIDE_POPUP_");
    view->inject_ime(prefix, QString());
    if (!popup || !await([&] { return popup->isVisible() && popup->topLevelItemCount() >= 2; })) {
        std::printf("[completion] caret popup did not appear\n"); return false;
    }
    ok &= pane_contains_popup() && view->hasFocus();
    key(view, Qt::Key_Down);
    const QString selected = popup->currentItem()->text(0);
    QEventLoop blink;
    QTimer::singleShot(1100, &blink, &QEventLoop::quit);
    blink.exec();
    const bool stable = popup->isVisible() && popup->currentItem() && popup->currentItem()->text(0) == selected;
    std::printf("[completion] selection survives idle cursor redraw: %s\n", stable ? "PASS" : "FAIL");
    if (!stable) return false;
    const auto revision = history_revision(view->tab());
    window.grab().save(QStringLiteral("native/target/terminal-session-splits.png"));
    popup->grab().save(QStringLiteral("native/target/terminal-caret-suggestions.png"));
    key(view, Qt::Key_Tab);
    ok &= await([&] { const auto s = terminal_completion_query(view->tab()); return QString::fromUtf8(s.query.data(), int(s.query.size())) == selected; });
    ok &= !popup->isVisible() && history_revision(view->tab()) == revision;
    std::printf("[completion] keyboard acceptance fills pending input without executing: %s\n", ok ? "PASS" : "FAIL");
    phase = "cancel-after-accept";
    key(view, Qt::Key_C, Qt::ControlModifier);
    if (!await([&] { return prompt(view); })) { diagnostic("cancel-or-submit-prompt"); return false; }
    view->inject_ime(prefix, QString());
    if (!await([&] { return popup->isVisible(); })) return false;
    phase = "second-accept-and-submit";
    const QString chosen = popup->currentItem()->text(0);
    const auto before = history_revision(view->tab());
    // Keep the first Enter held: an auto-repeat cannot become an execution.
    QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
    QApplication::sendEvent(view, &enter);
    key(view, Qt::Key_Return, Qt::NoModifier, true);
    QKeyEvent release(QEvent::KeyRelease, Qt::Key_Return, Qt::NoModifier);
    QApplication::sendEvent(view, &release);
    ok &= await([&] { const auto s = terminal_completion_query(view->tab()); return QString::fromUtf8(s.query.data(), int(s.query.size())) == chosen; });
    ok &= history_revision(view->tab()) == before;
    key(view, Qt::Key_Return);
    ok &= await([&] { return prompt(view) && history_revision(view->tab()) == before + 1; });
    std::printf("[completion] accepting Enter repeat does not execute; next fresh Enter executes once: %s\n", ok ? "PASS" : "FAIL");
    phase = "post-submit-suggestions";
    view->inject_ime(prefix, QString());
    if (!await([&] { return popup->isVisible(); })) return false;
    phase = "reading-transition";
    key(view, Qt::Key_Return, Qt::ControlModifier);
    const bool reading = await([&] { return term_reading(view->tab()) && !popup->isVisible(); });
    std::printf("[completion] Ctrl+Enter preserves reading mode instead of accepting a candidate: %s\n", reading ? "PASS" : "FAIL");
    if (!reading) return false;
    key(view, Qt::Key_Return, Qt::ControlModifier);
    if (!await([&] { return !term_reading(view->tab()); })) return false;
    phase = "reading-exit-suggestions";
    key(view, Qt::Key_Space, Qt::ControlModifier);
    if (!await([&] { return popup->isVisible(); })) return false;
    view->inject_ime(QString(), QStringLiteral("候选"));
    ok &= !popup->isVisible();
    // A mode transition resets the view's preedit without another IME event.
    // Suggestions must use that live state rather than remember a stale flag.
    phase = "ime-mode-reset";
    key(view, Qt::Key_Return, Qt::ControlModifier);
    if (!await([&] { return term_reading(view->tab()) && view->preedit().isEmpty(); })) return false;
    key(view, Qt::Key_Return, Qt::ControlModifier);
    if (!await([&] { return !term_reading(view->tab()); })) return false;
    key(view, Qt::Key_Space, Qt::ControlModifier);
    const bool ime_recovered = await([&] { return popup->isVisible(); });
    std::printf("[completion] suggestions recover after preedit is reset by reading mode: %d\n", ime_recovered);
    if (!ime_recovered) return false;
    // A layout change is a retirement boundary. The old list may hide, but it
    // must never survive at stale screen coordinates or outside the pane.
    const QSize previous_size = window.size();
    window.resize(previous_size - QSize(100, 50));
    QEventLoop resize_settle;
    QTimer::singleShot(250, &resize_settle, &QEventLoop::quit);
    resize_settle.exec();
    const bool retired_without_float = !popup->isVisible() || pane_contains_popup();
    std::printf("[completion] resize retires list without floating outside: %d\n", retired_without_float);
    if (!retired_without_float) return false;
    window.resize(previous_size);
    view->inject_ime(QString(), QString());
    window.focus_session(retained);
    ok &= !popup->isVisible();
    window.resize(qMin(1000, available.width()), qMin(650, available.height()));
    ok &= await([&] { window.refresh_chrome(); return layout(window); });
    phase = "final-layout-and-retirement";
    QPointer<PtyView> retired(view);
    window.close_pane(view); QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    ok &= retired.isNull() && window.workspace()->groups().size() == 2 && term_uid(retained) == uid;
    std::printf("[split] narrow nested groups, IME, focus and close retain complete live session ownership: %s\n", ok ? "PASS" : "FAIL");
    trace.succeeded = ok;
    return ok;
}
