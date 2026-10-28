// Distinct owners: C1 protects page/session lifetime; C3 protects dock controls.
#include "cpp/chrome/selftest_groups.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/panel_controls.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/panel_splits.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QDockWidget>
#include <QDrag>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QEventLoop>
#include <QKeyEvent>
#include <QInputMethodEvent>
#include <QLineEdit>
#include <QMenu>
#include <QMimeData>
#include <QMouseEvent>
#include <QSplitter>
#include <QSplitterHandle>
#include <QSettings>
#include <QPointer>
#include <QTabBar>
#include <QTabWidget>
#include <QToolButton>
#include <QShortcut>
#include <QTimer>
#include <QWindow>
#include <QElapsedTimer>
#include <QThread>
#include <QDoubleSpinBox>
#include <QSpinBox>
#include <cstdio>
#include <QAction>
#include <QLabel>
#include <QStatusBar>
#ifdef Q_OS_WIN
#include <QtCore/qt_windows.h>
#endif

// Qt's own QTest keyboard helper enters this QtGui event path. Plain
// sendEvent(QWidget, KeyPress) bypasses shortcut matching; no OS input is used.
Q_GUI_EXPORT bool qt_sendShortcutOverrideEvent(QObject *, ulong, int, Qt::KeyboardModifiers,
                                                const QString &, bool, ushort);

namespace {
void wake_fixture_drag(TabWorkspace *workspace) {
#ifdef Q_OS_WIN
    if (QGuiApplication::platformName() != QLatin1String("windows")) return;
    // OLE observes Qt's cancel flag in its next QueryContinueDrag call. A
    // synthetic Qt drag has no physical mouse message to wake that loop.
    // Post only to this process's capture window; never move the OS cursor.
    HWND receiver = GetCapture();
    if (!receiver) receiver = reinterpret_cast<HWND>(workspace->window()->winId());
    DWORD process = 0;
    GetWindowThreadProcessId(receiver, &process);
    const bool sent = process == GetCurrentProcessId() && PostMessageW(receiver, WM_MOUSEMOVE, 0, 0);
    std::fprintf(stderr, "[C1] owned native drag wake=%d\n", sent);
#else
    Q_UNUSED(workspace);
#endif
}
void cancel_fixture_drag(TabWorkspace *workspace) {
    QDrag::cancel();
    wake_fixture_drag(workspace);
}
bool drag_tab(TabWorkspace *workspace, QTabWidget *source, int at, QTabWidget *target, bool whole_group = false) {
    auto *bar = source->tabBar();
    QWidget *handle = bar;
    if (whole_group) for (auto *button : source->cornerWidget()->findChildren<QToolButton *>())
        if (button->property("groupHandle").toBool()) handle = button;
    const QPoint start = whole_group ? handle->rect().center() : bar->tabRect(at).center();
    QMouseEvent press(QEvent::MouseButtonPress, start, handle->mapToGlobal(start),
        Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QApplication::sendEvent(handle, &press);
    bool delivered = false;
    // Exercise the production QDrag's MIME and actual target event filter. The
    // offscreen platform still runs Qt's nested drag loop, without OS input.
    QTimer::singleShot(0, workspace, [&] {
        const auto drags = workspace->findChildren<QDrag *>();
        if (drags.isEmpty()) return;
        auto *drag = drags.last();
        const auto fields = drag->mimeData()->data("application/x-tideterm-tab-group").split('\n');
        if (fields.size() != 3 || (whole_group && fields.last() != "-1")) { cancel_fixture_drag(workspace); return; }
        const QPoint point = target->rect().center();
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
        QDragEnterEvent enter(QPointF(point), Qt::MoveAction, drag->mimeData(), Qt::LeftButton, Qt::NoModifier);
#else
        QDragEnterEvent enter(point, Qt::MoveAction, drag->mimeData(), Qt::LeftButton, Qt::NoModifier);
#endif
        QApplication::sendEvent(target, &enter);
        QDropEvent drop(point, Qt::MoveAction, drag->mimeData(), Qt::LeftButton, Qt::NoModifier);
        QApplication::sendEvent(target, &drop);
        delivered = enter.isAccepted() && drop.isAccepted();
        cancel_fixture_drag(workspace);
    });
    const QPoint outside(start.x(), handle->height() + 40);
    QMouseEvent move(QEvent::MouseMove, outside, handle->mapToGlobal(outside),
        Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    wake_fixture_drag(workspace);
    QApplication::sendEvent(handle, &move);
    QCoreApplication::processEvents();
    return delivered;
}
}

bool verify_tab_groups(MainWindow *window) {
    auto *workspace = window->workspace();
    auto *first = workspace->first_group();
    auto *retained = window->pane_view(2, 0);
    const uint32_t retained_id = retained->tab();
    const rust::String uid = term_uid(retained_id);
    window->focus_session(retained_id);
    auto *horizontal = chrome::find_menu_action(window->menuBar(), QStringLiteral("window:split-group-horizontal"));
    horizontal->trigger();
    QCoreApplication::processEvents(); // Reparented pages must finish their show/layout events.
    bool ok = workspace->groups().size() == 2 && term_tab_count() == 3;
    auto *second = workspace->groups().last();
    ok &= second != first && retained->isVisible() && window->active_view() == retained;
    window->focus_session(window->page_id(1));
    auto *vertical = chrome::find_menu_action(window->menuBar(), QStringLiteral("window:split-group-vertical"));
    vertical->trigger();
    QCoreApplication::processEvents();
    auto *third = workspace->groups().last();
    ok &= workspace->groups().size() == 3 && term_tab_count() == 3;
    auto *moved_page = third->currentWidget();
    // Focusing chrome of another group must bind the window to that group.
    // 2026-10-01：连接栏地址改为只读 QLabel（WindTerm 地址栏样式，不可聚焦），
    // 原 `page_banner(1)->findChild<QLineEdit*>()` 会取到空指针。改用组内终端窗格
    // 聚焦——与用户点终端同一条 focusChanged 路径。
    window->focus_session(retained_id);
    window->pane_view(1, 0)->setFocus();
    QCoreApplication::processEvents();
    ok &= window->current_page() == 1 && window->active_view() == window->pane_view(1, 0);
    window->grab().save(QStringLiteral("native/target/groups-independent.png"));
    for (auto *group : workspace->groups()) {
        const auto pos = group->mapTo(window, QPoint());
        std::printf("[cpp] C1 group %s visible=%d position=%d,%d size=%dx%d workspace=%dx%d\n",
            workspace->group_name(group).toUtf8().constData(), group->isVisible(), pos.x(), pos.y(),
            group->width(), group->height(), workspace->width(), workspace->height());
        ok &= group->isVisible() && group->width() > 100 && group->height() > 100;
        ok &= QRect(workspace->mapTo(window, QPoint()), workspace->size()).contains(QRect(pos, group->size()));
    }
    const bool dropped = drag_tab(workspace, third, 0, second);
    ok &= dropped && workspace->owner(moved_page) == second && workspace->groups().size() == 2;
    const rust::String after_uid = term_uid(retained_id);
    ok &= std::string(uid.data(), uid.size()) == std::string(after_uid.data(), after_uid.size());
    window->activate_group(second);
    chrome::find_menu_action(window->menuBar(), QStringLiteral("window:merge-to-group"))->trigger();
    QCoreApplication::processEvents();
    ok &= workspace->groups().size() == 1 && first->count() == 3 && term_tab_count() == 3;
    auto *recreated = workspace->split_group(first, Qt::Vertical);
    QWidget *retained_page = first->currentWidget();
    workspace->move_page(moved_page, recreated);
    // Move a second distinct page so a tab drag cannot masquerade as a group drag.
    if (retained_page == moved_page) retained_page = first->widget(0);
    workspace->move_page(retained_page, recreated);
    const bool group_dropped = drag_tab(workspace, recreated, 0, first, true);
    ok &= group_dropped && workspace->groups().size() == 1 && first->count() == 3 &&
        workspace->owner(moved_page) == first && workspace->owner(retained_page) == first;
    QCoreApplication::processEvents();
    window->grab();
    ok &= first->isVisible() && first->width() > 100 && first->height() > 100 && retained->painted() > 0;
    ok &= window->pane_view(2, 0) == retained && window->page_sendbar(2) != nullptr;
    std::printf("[cpp] C1 independent groups: tab drag=%d group drag=%d preserved process/pages=%s\n", dropped, group_dropped, ok ? "PASS" : "FAIL");
    return ok;
}

/// 面板分割自检：手柄热区/自绘、拖拽比例钳、比例持久化往返。
/// 用独立的横向分割器（面板本体只有 310 px 宽，切不出可见比例）。
bool verify_panel_splits() {
    bool ok = true;
    constexpr int kSpan = 500;
    constexpr int kHandle = 5;
    // ① 手柄：面板分割器自带 5px 握持区（中央画 1px 主题线），居中线落在分割器坐标中心。
    QSplitter *sp = chrome::panel_splitter(Qt::Horizontal);
    sp->setObjectName(QStringLiteral("panelSplitRatioProbe"));
    chrome::install_panel_split(sp);
    sp->setAttribute(Qt::WA_DontShowOnScreen, true);
    auto *left = new QWidget;
    auto *right = new QWidget;
    left->setMinimumSize(0, 0);
    right->setMinimumSize(0, 0);
    sp->addWidget(left);
    sp->addWidget(right);
    sp->resize(kSpan, 120);
    sp->show();
    QCoreApplication::processEvents();
    // 断言比对真实手柄几何的中心，而不是写死 248/249：手柄矩形由 Qt 在 (total-handle)/2
    // 处取整展开（500/5 → x=247、宽 5 ⇒ 中心 250），面板分割器的硬要求只是「自绘线落在
    // 分割器正中」，写死数字会在手柄宽度或总长变化时假失败。
    const bool handle_ok = sp->handleWidth() == chrome::panel_split_handle_width() &&
                           sp->handle(1)->width() == kHandle &&
                           sp->handle(1)->geometry().center().x() == kSpan / 2;
    ok &= handle_ok;
    std::printf("[cpp] C3 panel split handle=%d real=%d center=%d\n", sp->handleWidth(),
        sp->handle(1)->width(), sp->handle(1)->geometry().center().x());
    // ② 拖拽钳位：未接线的分割器会一路拖到 0（塌陷），接线的停在 10%/90%。
    const auto drag = [sp](int target) {
        auto *handle = sp->handle(1);
        const QPoint origin = handle->rect().center();
        const QPoint global_origin = handle->mapToGlobal(origin);
        const QPoint global_goal = sp->mapToGlobal(QPoint(target, 60));
        QMouseEvent press(QEvent::MouseButtonPress, origin, global_origin, Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
        QApplication::sendEvent(handle, &press);
        QMouseEvent move(QEvent::MouseMove, handle->mapFromGlobal(global_goal), global_goal,
            Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
        QApplication::sendEvent(handle, &move);
        QMouseEvent release(QEvent::MouseButtonRelease, handle->mapFromGlobal(global_goal), global_goal,
            Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
        QApplication::sendEvent(handle, &release);
        QCoreApplication::processEvents();
    };
    drag(0);
    const QList<int> low = sp->sizes();
    const bool low_ok = !sp->childrenCollapsible() && low.size() == 2 && low.at(0) > 0 &&
                        qAbs(low.at(0) - kSpan / 10) <= 3 && low.at(0) + low.at(1) > 0;
    ok &= low_ok;
    std::printf("[cpp] C3 panel split drag low=%d clamps=%d/%d\n", low.at(0), chrome::clamp_split_a(kSpan, 0),
        kSpan - chrome::clamp_split_a(kSpan, 0));
    drag(kSpan);
    const QList<int> high = sp->sizes();
    // 上限受右窗格 min（= 10% = 50）压住：拖到最右停在 500-5-50 = 445，而非 450。
    const bool high_ok = high.size() == 2 && qAbs(high.at(0) - (kSpan - kHandle - kSpan / 10)) <= 3 &&
                         high.at(1) >= kSpan / 10 - 1;
    ok &= high_ok;
    std::printf("[cpp] C3 panel split drag high=%d min=%d clamps=%d\n", high.at(0), high.at(1),
        chrome::clamp_split_a(kSpan, kSpan));
    const bool drag_ok = low_ok && high_ok;
    // ③ 纯函数：|上| + |下| 恒等于总长，越界回钳。
    const chrome::SplitClick mid = chrome::panel_split_states(kSpan, 200);
    const chrome::SplitClick edge = chrome::panel_split_states(kSpan, kSpan);
    const bool states_ok = mid.upper == 200 && mid.lower == 300 && edge.upper == kSpan &&
                           edge.lower == 0 && chrome::panel_split_states(0, 3).upper == 0;
    ok &= states_ok;
    // ④ 持久化：保存 → 改尺寸 → 恢复；同一轮里验证取向变化会被跳过。
    // groups = 分割器名 → 比例值。QSettings 会缓存读到的表，所以**保存之后再现构造**，
    // 否则读到的是保存前那份（旧版本在这里假失败过）。
    // 注意 `s` 不能是 const：`allKeys()` 的非 const 重载才会先 sync()，const 重载只回
    // 本地缓存 —— 用 const QSettings 读刚写进注册表的值恒定读到空表（这里踩过）。
    const auto read_splits = [](const char *group) {
        QSettings s(QSettings::defaultFormat(), QSettings::UserScope,
                    QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
        QMap<QString, QString> found;
        const QString prefix = QString::fromLatin1(group);
        for (const QString &key : s.allKeys())
            if (key.startsWith(prefix)) found.insert(key.mid(prefix.size()), s.value(key).toString());
        return found;
    };
    // 组前缀自带结尾的 '/'（`layout/panelSplits.v1/`）：lambda 只做 startsWith，
    // 再补一个 '/' 会变成 `...v1//` 而永远匹配不到任何键（这里踩过）。
    constexpr char kSplitGroup[] = "layout/panelSplits.v1/";
    {
        QSettings wipe(QSettings::defaultFormat(), QSettings::UserScope,
                       QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
        wipe.remove(QString::fromLatin1(kSplitGroup)); // 清掉上一轮的组，避免假阳性
        wipe.sync();
    }
    chrome::save_panel_splits();
    const QMap<QString, QString> after_save = read_splits(kSplitGroup);
    // 键是「链路名 → 比例」，值是 `'h' + 0x1f + 十六进制状态`。链路名逐级往下拼，
    // 每级**有 objectName 就用它、没有才退回类名**，所以探针（设了 objectName）就是
    // `panelSplitProbe` 本身，而生产里那些匿名分割器才是 `QMainWindow/dockFiles/.../QSplitter`。
    // 断言只能按后缀认领自己那条：链路名含全部祖先，同一窗口里多个分割器才各有各的键。
    const auto find_by_suffix = [](const QMap<QString, QString> &table, const QString &suffix) {
        for (auto it = table.constBegin(); it != table.constEnd(); ++it)
            if (it.key().endsWith(suffix)) return it.value();
        return QString();
    };
    // 后缀必须与本文件另一个探针（`panelSplitFlip`）**互不为后缀**：两个探针一次运行里
    // 都进注册表，若名字套名字，按后缀认领会先撞上对方那条，拿到的就不是自己的比例。
    const QString own_suffix = QStringLiteral("panelSplitRatioProbe");
    const QString own_value = find_by_suffix(after_save, own_suffix);
    const QStringList own = own_value.split(QChar(0x1f));
    // 值 = 轴向字符 + 0x1f + `saveState().toHex()` ⇒ 正好 2 段（实测：'h' + 62 个十六进制
    // 字符 = 64 字符）。别把段数写成 3：toHex 不会再插入分隔符。
    const bool stored_ok = !own_value.isEmpty() && own.size() == 2 &&
                           own.at(0) == QStringLiteral("h") && own.at(1).size() > 8;
    ok &= stored_ok;
    std::printf("[cpp] C3 panel split stored value-parts=%lld groups=%lld\n",
        static_cast<long long>(own.size()), static_cast<long long>(after_save.size()));
    // 陈旧清理：上一轮写过、这一轮不存在的分割器键必须被删掉（曾经这段清理因为拿相对键去
    // 比绝对前缀而整段空转，死键会一直留在注册表里）。
    {
        QSettings stale(QSettings::defaultFormat(), QSettings::UserScope,
                        QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
        stale.setValue(QStringLiteral("layout/panelSplits.v1/QSplitter#staleProbe"),
            QStringLiteral("h") + QChar(0x1f) + QStringLiteral("00"));
    }
    chrome::save_panel_splits();
    const bool stale_ok = find_by_suffix(read_splits(kSplitGroup), QStringLiteral("staleProbe")).isEmpty() &&
                          !own_value.isEmpty();
    // 第二个分割器：存的是竖排，恢复时已翻成横排 ⇒ 必须整个跳过，尺寸不动。
    QSplitter *flip = chrome::panel_splitter(Qt::Vertical);
    flip->setObjectName(QStringLiteral("panelSplitFlip"));
    flip->setAttribute(Qt::WA_DontShowOnScreen, true);
    flip->addWidget(new QWidget);
    flip->addWidget(new QWidget);
    chrome::install_panel_split(flip);
    chrome::save_panel_splits();
    flip->setOrientation(Qt::Horizontal);
    flip->resize(kSpan, 120);
    // 往返断言的基准必须是**保存那一刻的实际尺寸**。这里不能用 `setSizes(300,200)` 当基准：
    // 探针挂着 WA_DontShowOnScreen，setSizes 在这种没真正上屏的分割器上不生效（请求 300/200
    // 之后 sizes() 立刻回落到拖拽留下的 297/198），于是「基准」根本不是恢复前那一份，断言
    // 必然假失败。改成「先收下当前尺寸 → 程序性破坏比例 → restore 必须复原成刚才那份」。
    const QList<int> baked = sp->sizes();
    sp->setSizes({300, 200});
    const bool disturbed = sp->sizes() != baked;
    // 第二个分割器：存的是竖排，恢复时已翻成横排 ⇒ 必须整个跳过，尺寸不动。
    const QList<int> flip_before = flip->sizes();
    chrome::restore_panel_splits();
    const bool round_trip = sp->sizes() == baked;
    const bool flip_ok = flip->sizes() == flip_before;
    ok &= round_trip && flip_ok && disturbed;
    std::printf("[cpp] C3 panel split persist stored=%d stale-cleanup=%d roundtrip=%d orientation-skip=%d\n",
        stored_ok ? 1 : 0, stale_ok ? 1 : 0, (round_trip && disturbed) ? 1 : 0, flip_ok ? 1 : 0);
    delete sp;
    delete flip;
    std::printf("[cpp] C3 panel splits: clamp=%d states=%d persist=%d: %s\n", drag_ok ? 1 : 0,
        states_ok ? 1 : 0,
        (stored_ok && stale_ok && round_trip && flip_ok && disturbed) ? 1 : 0, ok ? "PASS" : "FAIL");
    return ok;
}

bool verify_panel_controls(MainWindow *window) {
    auto *d = window->docks();
    const auto initial_terms = term_tab_count();
    const int initial_pages = window->page_count();
    bool ok = !d->hist->isVisible();
    QMenu menu;
    chrome::fill_panel_menu(&menu, d->sess);
    QMenu *split = menu.actions().front()->menu();
    QMenu *vertical = split->actions().front()->menu();
    QAction *history = nullptr;
    for (auto *action : vertical->actions()) if (action->text() == d->history->windowTitle()) history = action;
    if (!history) return false;
    history->trigger();
    QCoreApplication::processEvents();
    const bool menu_ok = d->sess->isVisible() && d->history->isVisible() &&
          !window->tabifiedDockWidgets(d->sess).contains(d->history);
    ok &= menu_ok;
    const bool drag_ok = verify_panel_drag(window);
    ok &= drag_ok;
    // Keyboard split from Sessions moves the next real panel (Shell), not a copy.
    d->sess->raise();
    window->activateWindow();
    d->sess->widget()->setFocus();
    QCoreApplication::processEvents();
    QString focused_panel;
    for (auto *widget = QApplication::focusWidget(); widget; widget = widget->parentWidget())
        if (auto *dock = qobject_cast<QDockWidget *>(widget)) { focused_panel = dock->objectName(); break; }
    bool activated = false;
    QObject observer;
    for (auto *shortcut : window->findChildren<QShortcut *>())
        if (shortcut->key() == QKeySequence(QStringLiteral("Ctrl+Alt+Shift+V")))
            QObject::connect(shortcut, &QShortcut::activated, &observer, [&] { activated = true; });
    const bool handled = qt_sendShortcutOverrideEvent(d->sess->widget(), 0, Qt::Key_V,
        Qt::ControlModifier | Qt::AltModifier | Qt::ShiftModifier, QStringLiteral("V"), false, 1);
    QCoreApplication::processEvents();
    const bool shortcut_ok = d->shell->isVisible() && !window->tabifiedDockWidgets(d->sess).contains(d->shell);
    ok &= shortcut_ok;
    const auto auxiliary = MainWindow::views(d->shell->widget());
    bool shell_ok = auxiliary.size() == 1 && term_tab_count() == initial_terms + 1;
    if (!auxiliary.empty()) {
        auto *view = auxiliary.front();
        view->setFocus();
        const bool focused = window->active_view() == view;
        shell_ok &= focused;
        // Observe the actual interactive prompt before typing into a newly started ConPTY.
        QElapsedTimer startup;
        startup.start();
        bool ready = false;
        while (startup.elapsed() < 6000 && !ready) {
            QCoreApplication::processEvents();
            const auto bytes = term_screen_text(view->tab());
            const QString screen = QString::fromUtf8(bytes.data(), int(bytes.size()));
#ifdef Q_OS_WIN
            ready = screen.contains(QStringLiteral("PS "));
#else
            ready = !screen.trimmed().isEmpty();
#endif
            QThread::msleep(10);
        }
        shell_ok &= ready;
#ifdef Q_OS_WIN
        const QString command = QStringLiteral("Write-Output ('SHELL_' + 'DOCK_OK')");
#else
        const QString command = QStringLiteral("printf '%s%s\\n' 'SHELL_' 'DOCK_OK'");
#endif
        QInputMethodEvent input;
        input.setCommitString(command);
        QApplication::sendEvent(view, &input);
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QApplication::sendEvent(view, &enter);
        QElapsedTimer elapsed;
        elapsed.start();
        bool echoed = false;
        while (elapsed.elapsed() < 6000 && !echoed) {
            QCoreApplication::processEvents();
            const auto text = term_screen_text(view->tab());
            echoed = QString::fromUtf8(text.data(), int(text.size())).contains(QStringLiteral("SHELL_DOCK_OK"));
            QThread::msleep(10);
        }
        shell_ok &= echoed && view->painted() > 0;
        std::printf("[cpp] C3 Shell focused=%d ready=%d echoed=%d painted=%u\n", focused, ready, echoed, view->painted());
        if (!echoed) {
            const auto stats = term_stats(view->tab());
            const auto screen = term_screen_text(view->tab());
            std::printf("[cpp] C3 Shell grid=%ux%u screen=[%.*s]\n", stats.cols, stats.rows, int(screen.size()), screen.data());
        }
        if (initial_pages == 1) {
            window->close_tab(0);
            view->setFocus();
            window->refresh_status();
            auto *status = window->statusBar()->findChild<QLabel *>(QStringLiteral("statusShell"));
            shell_ok &= window->page_count() == 0 && status && !status->text().isEmpty();
            std::printf("[cpp] C3 Shell without page: pages=%d status=%s\n", window->page_count(),
                status ? status->text().toUtf8().constData() : "missing");
            window->add_tab();
            view->setFocus();
        }
        chrome::find_menu_action(window->menuBar(), QStringLiteral("session:close-view"))->trigger();
        shell_ok &= term_tab_count() == initial_terms && window->page_count() == initial_pages;
        std::printf("[cpp] C3 Shell close: terms=%u/%u pages=%d/%d\n", term_tab_count(), initial_terms,
            window->page_count(), initial_pages);
    }
    ok &= shell_ok;
    std::printf("[cpp] C3 auxiliary Shell input/output/focus/close: %s\n", shell_ok ? "PASS" : "FAIL");
    // 面板分割自检放在窗口类断言**之后**：它要清空并重写 QSettings，还会建/删顶层分割器，
    // 先跑会打乱 dock 布局（历史现象：Shell 段的可见性断言被搅成假失败）。
    const bool splits_ok = verify_panel_splits();
    ok &= splits_ok;
    // 这一行只报告 dock 当前状态（Shell 段已按测试流程把它关掉，可见必为 0），不是判定依据。
    std::printf("[cpp] C3 shortcut source=%s Shell visible=%d hidden=%d area=%d peers=%lld\n",
        focused_panel.toUtf8().constData(), d->shell->isVisible(), d->shell->isHidden(),
        int(window->dockWidgetArea(d->shell)), window->tabifiedDockWidgets(d->sess).size());
    std::printf("[cpp] C3 panel controls: menu=%d drag=%d shortcut=%d shell=%d handled=%d activated=%d focus=%s: %s\n", menu_ok, drag_ok, shortcut_ok, shell_ok, handled, activated,
        QApplication::focusWidget() ? QApplication::focusWidget()->metaObject()->className() : "none", ok ? "PASS" : "FAIL");
    return ok;
}

bool verify_send_pacing() {
    std::vector<std::string> writes;
    SendBar sender;
    sender.set_writer_factory([&](int) -> SendBar::Writer {
        return [&](const std::string &bytes, bool) { writes.push_back(bytes); return true; };
    }, {});
    sender.findChild<QAction *>(QStringLiteral("sendCharacters"))->trigger();
    sender.findChild<QSpinBox *>()->setValue(2);
    for (auto *interval : sender.findChildren<QDoubleSpinBox *>()) interval->setValue(0.1);
    sender.findChild<QDoubleSpinBox *>(QStringLiteral("sendCharacterInterval"))->setValue(0.01);
    sender.input()->setText(QStringLiteral("A中e\u0301"));
    sender.send();
    QElapsedTimer elapsed;
    elapsed.start();
    while (sender.sent_count() < 2 && elapsed.elapsed() < 2000) {
        QCoreApplication::processEvents(); QThread::msleep(5);
    }
    const std::vector<std::string> expected = {"A", QStringLiteral("中").toUtf8().toStdString(),
        QStringLiteral("e\u0301").toUtf8().toStdString(), "\r", "A", QStringLiteral("中").toUtf8().toStdString(),
        QStringLiteral("e\u0301").toUtf8().toStdString(), "\r"};
    bool ok = writes == expected && sender.sent_count() == 2;
    if (!ok) {
        std::printf("[cpp] C3 pacing writes=%zu sent=%u\n", writes.size(), sender.sent_count());
        for (const auto &bytes : writes) std::printf("[cpp] C3 pacing byte chunk=%s\n", QByteArray(bytes.data(), int(bytes.size())).toHex().constData());
    }
    writes.clear();
    sender.input()->setText(QStringLiteral("ABC"));
    sender.send();
    for (auto *button : sender.findChildren<QToolButton *>())
        if (button->toolTip() == i18n::tr("停止发送")) button->click();
    elapsed.restart();
    while (elapsed.elapsed() < 80) { QCoreApplication::processEvents(); QThread::msleep(5); }
    ok &= writes == std::vector<std::string>{"A"} && sender.sent_count() == 2;
    std::printf("[cpp] C3 character pacing UTF-8/repeat/stop: %s\n", ok ? "PASS" : "FAIL");
    return ok;
}
