// User-facing regressions: long notifications, transient file scrolling and window actions.
#include "cpp/chrome/selftest_chrome_layout.h"
#include "cpp/chrome/status_bar.h"
#include "cpp/chrome/tab_buttons.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/session_form.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"

#include <QApplication>
#include <QElapsedTimer>
#include <QDir>
#include <QEnterEvent>
#include <QFile>
#include <QImage>
#include <QPixmap>
#include <QLabel>
#include <QLineEdit>
#include <QKeyEvent>
#include <QDialog>
#include <QComboBox>
#include <QStackedWidget>
#include <QPushButton>
#include <QTimer>
#include <QMouseEvent>
#include <QMenuBar>
#include <QScrollBar>
#include <QStatusBar>
#include <QTabBar>
#include <QTabWidget>
#include <QStyleOptionSlider>
#include <QTemporaryDir>
#include <QThread>
#include <QToolButton>
#include <QTreeWidget>
#include <QWheelEvent>
#include <cstdio>

namespace {
// Inspect painted ink, not the outer label/button geometry. No renderer-derived expectations.
QRect ink_bounds(const QImage &image, const QRectF &logical, qreal dpr) {
    const QRect box = QRectF(logical.topLeft() * dpr, logical.size() * dpr).toAlignedRect().intersected(image.rect());
    if (box.isEmpty()) return {};
    const QColor background = image.pixelColor(box.left(), qMin(image.height() - 1, qRound(4 * dpr)));
    QRect ink;
    for (int y = qMax(box.top(), qRound(4 * dpr)); y <= qMin(box.bottom(), image.height() - qRound(3 * dpr)); ++y) {
        for (int x = box.left(); x <= box.right(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            const int contrast = qMax(qAbs(pixel.red() - background.red()),
                qMax(qAbs(pixel.green() - background.green()), qAbs(pixel.blue() - background.blue())));
            if (contrast > 40) ink = ink.united(QRect(x, y, 1, 1));
        }
    }
    return ink;
}

bool painted_tab_alignment(QTabBar *bar) {
    const QPixmap painted = bar->grab();
    const QImage image = painted.toImage();
    const qreal dpr = painted.devicePixelRatio();
    bool ok = true;
    for (int i = 0; i < bar->count(); ++i) {
        auto *lead = bar->tabButton(i, QTabBar::LeftSide);
        auto *chip = lead ? lead->findChild<QLabel *>(QStringLiteral("tabActivitySquare")) : nullptr;
        auto *tail = bar->tabButton(i, QTabBar::RightSide);
        auto *close = tail ? tail->findChild<QToolButton *>(QStringLiteral("tabClose")) : nullptr;
        if (!chip || !close) { ok = false; continue; }
        const QRectF tab(bar->tabRect(i));
        const QRectF icon_box(chip->mapTo(bar, QPoint()), chip->size());
        const QRectF close_box(close->mapTo(bar, QPoint()), close->size());
        const QRectF text_box(QPointF(lead->geometry().right() + 1, tab.y()),
                             QSizeF(tail->x() - lead->geometry().right() - 2, tab.height()));
        const QRect icon_ink = ink_bounds(image, icon_box, dpr);
        const QRect text_ink = ink_bounds(image, text_box, dpr);
        const QRect close_ink = ink_bounds(image, close_box, dpr);
        const auto center = [](QRect r) { return (r.top() + r.bottom()) / 2.0; };
        ok &= !icon_ink.isEmpty() && !text_ink.isEmpty() && !close_ink.isEmpty() &&
              qAbs(center(text_ink) - center(icon_ink)) <= 1 && qAbs(center(close_ink) - center(icon_ink)) <= 1;
        std::printf("[cpp] tab ink %d title=%s dpr=%g text=%g icon=%g close=%g\n", i,
                    bar->tabText(i).toUtf8().constData(), dpr, center(text_ink), center(icon_ink), center(close_ink));
    }
    return ok;
}

bool painted_group_alignment(QTabBar *bar) {
    auto *group = dynamic_cast<QTabWidget *>(bar->parentWidget());
    auto *corner = group ? group->cornerWidget() : nullptr;
    if (!corner || bar->currentIndex() < 0) return false;
    const auto tab = bar->tabRect(bar->currentIndex());
    const auto left = bar->grab(); const auto right = corner->grab();
    const qreal dpr = left.devicePixelRatio();
    auto *lead = bar->tabButton(bar->currentIndex(), QTabBar::LeftSide);
    auto *tail = bar->tabButton(bar->currentIndex(), QTabBar::RightSide);
    if (!lead || !tail) return false;
    const QRect text = ink_bounds(left.toImage(), QRectF(lead->geometry().right() + 1, tab.y(),
        tail->x() - lead->geometry().right() - 2, tab.height()), dpr);
    const qreal center = bar->mapToGlobal(QPoint()).y() * dpr + (text.top() + text.bottom()) / 2.0;
    bool ok = !text.isEmpty();
    for (auto *button : corner->findChildren<QToolButton *>()) {
        const QRect ink = ink_bounds(right.toImage(), QRectF(button->geometry()), dpr);
        const qreal actual = corner->mapToGlobal(QPoint()).y() * dpr + (ink.top() + ink.bottom()) / 2.0;
        ok &= !ink.isEmpty() && qAbs(actual - center) <= 1;
        std::printf("[cpp] group/tab ink dpr=%g tab=%g control=%g\n", dpr, center, actual);
    }
    return ok;
}

void settle(int ms = 50) {
    QElapsedTimer elapsed;
    elapsed.start();
    do {
        QApplication::processEvents();
        if (ms > 0) QThread::msleep(5);
    } while (elapsed.elapsed() < ms);
}

template<class Ready> void settle_until(Ready ready) {
    QElapsedTimer deadline;
    deadline.start();
    do { settle(10); } while (!ready() && deadline.elapsed() < 2000);
}

bool notification_bounds(MainWindow &window) {
    const QSize previous = window.size();
    window.resize(900, 650);
    settle();
    const QSize compact = window.size();
    const QString message = QStringLiteral("通知：/a/very/long/path/").repeated(80);
    window.note(message);
    settle();
    auto *label = window.statusBar()->findChild<QLabel *>(QStringLiteral("statusMsg"));
    bool ok = window.size() == compact && window.minimumSizeHint().width() <= compact.width() &&
              label && label->text() == message;
    for (auto *part : window.statusBar()->findChildren<QLabel *>()) {
        if (!part->isVisible()) continue;
        const QRect bounds(part->mapTo(window.statusBar(), QPoint()), part->size());
        ok &= window.statusBar()->rect().contains(bounds);
    }
    chrome::status_note(window.statusBar(), QString());
    window.resize(previous);
    settle();
    return ok;
}

bool file_scroll_interaction() {
    QTemporaryDir directory;
    if (!directory.isValid()) {
        std::printf("[cpp] file fixture directory: %s (temp=%s)\n", qPrintable(directory.errorString()), qPrintable(QDir::tempPath()));
        return false;
    }
    for (int row = 0; row < 80; ++row) {
        QFile file(directory.filePath(QStringLiteral("file-%1.txt").arg(row, 3, 10, QLatin1Char('0'))));
        if (!file.open(QIODevice::WriteOnly)) {
            std::printf("[cpp] file fixture write: %s (%s)\n", qPrintable(file.errorString()), qPrintable(file.fileName()));
            return false;
        }
        if (row < 2) file.write(QByteArray(row == 0 ? 2 : 10, 'x'));
    }
    if (!QDir(directory.path()).mkdir(QStringLiteral("folder"))) return false;
    SftpBrowser browser;
    browser.follow_terminal(0, 0, directory.path());
    browser.resize(340, 240);
    browser.show();
    QElapsedTimer elapsed;
    elapsed.start();
    while (browser.local_rows() != 81 && elapsed.elapsed() < 3000) {
        browser.poll();
        settle(10);
    }
    auto *tree = browser.local_tree();
    auto *source = tree->verticalScrollBar();
    tree->sortItems(3, Qt::DescendingOrder);
    const bool sorted = tree->topLevelItemCount() == 81 &&
        tree->topLevelItem(0)->text(0) == QStringLiteral("folder") &&
        tree->topLevelItem(1)->text(0) == QStringLiteral("file-001.txt");
    tree->sortItems(0, Qt::AscendingOrder);
    settle(chrome::metrics::kFileScrollHideDelay + 100);
    bool idle = true;
    for (auto *bar : tree->findChildren<QScrollBar *>()) idle &= !bar->isVisible();
    const int before = source->value();
    const QPointF hover_position(tree->viewport()->rect().center());
    QEnterEvent hover(hover_position, hover_position, tree->viewport()->mapToGlobal(hover_position.toPoint()));
    QApplication::sendEvent(tree->viewport(), &hover);
    settle();
    bool hovered = false;
    for (auto *bar : tree->findChildren<QScrollBar *>())
        hovered |= bar->isVisible() && bar->orientation() == Qt::Vertical;
    const QPointF position(tree->viewport()->rect().center());
    QWheelEvent wheel(position, tree->viewport()->mapToGlobal(position.toPoint()), QPoint(),
                      QPoint(0, -120), Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(tree->viewport(), &wheel);
    settle();
    QScrollBar *visible = nullptr;
    for (auto *bar : tree->findChildren<QScrollBar *>()) {
        if (bar->isVisible() && bar->orientation() == Qt::Vertical) visible = bar;
    }
    bool ok = browser.local_rows() == 81 && sorted && idle && hovered && source->value() > before && visible;
    std::printf("[cpp] file scroll rows=%d idle=%d before=%d after=%d max=%d visible=%d\n",
                browser.local_rows(), int(idle), before, source->value(), source->maximum(), int(visible != nullptr));
    if (visible) {
        QStyleOptionSlider option;
        option.initFrom(visible);
        option.orientation = visible->orientation();
        option.minimum = visible->minimum();
        option.maximum = visible->maximum();
        option.pageStep = visible->pageStep();
        option.sliderPosition = option.sliderValue = visible->value();
        const QRect thumb = visible->style()->subControlRect(QStyle::CC_ScrollBar, &option,
                                                             QStyle::SC_ScrollBarSlider, visible);
        const QPointF start(thumb.center());
        const QPointF end(start.x(), visible->height() - 1);
        auto mouse = [visible](QEvent::Type type, const QPointF &position, Qt::MouseButton button,
                                Qt::MouseButtons buttons) {
            QMouseEvent event(type, position, visible->mapToGlobal(position.toPoint()), button,
                              buttons, Qt::NoModifier);
            QApplication::sendEvent(visible, &event);
        };
        mouse(QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
        mouse(QEvent::MouseMove, end, Qt::NoButton, Qt::LeftButton);
        mouse(QEvent::MouseButtonRelease, end, Qt::LeftButton, Qt::NoButton);
        settle();
        const QRect bounds(tree->viewport()->mapFromGlobal(visible->mapToGlobal(QPoint())), visible->size());
        ok &= source->value() == source->maximum() && tree->viewport()->rect().contains(bounds);
        std::printf("[cpp] file scroll source=%d proxy=%d bounds=%d\n", source->value(), visible->value(),
                    int(tree->viewport()->rect().contains(bounds)));
    }
    settle(chrome::metrics::kFileScrollHideDelay + 100);
    settle_until([&] {
        for (auto *bar : tree->findChildren<QScrollBar *>()) if (bar->isVisible()) return false;
        return true;
    });
    for (auto *bar : tree->findChildren<QScrollBar *>()) {
        ok &= !bar->isVisible();
        if (bar->isVisible()) std::printf("[cpp] scroll remains visible: %s\n", qPrintable(bar->objectName()));
    }
    return ok;
}

bool window_actions(MainWindow &window) {
    auto *maximize = window.menuBar()->findChild<QToolButton *>(QStringLiteral("windowMaximize"));
    auto *minimize = window.menuBar()->findChild<QToolButton *>(QStringLiteral("windowMinimize"));
    auto *close = window.menuBar()->findChild<QToolButton *>(QStringLiteral("windowClose"));
    if (!maximize || !minimize || !close) {
        std::printf("[cpp] window controls found=%d/%d/%d\n", int(maximize != nullptr), int(minimize != nullptr), int(close != nullptr));
        return false;
    }
    window.showNormal();
    settle();
    const QRect normal = window.geometry();
    maximize->click();
    settle_until([&] { return window.isMaximized() && window.geometry() != normal; });
    bool ok = window.isMaximized();
    std::printf("[cpp] window maximized=%d\n", int(ok));
    maximize->click();
    settle_until([&] { return !window.isMaximized() && window.geometry() == normal; });
    ok &= !window.isMaximized() && window.geometry() == normal;
    std::printf("[cpp] window restored=%d geometry=%d (%d,%d %dx%d -> %d,%d %dx%d)\n",
                int(!window.isMaximized()), int(window.geometry() == normal), normal.x(), normal.y(), normal.width(), normal.height(),
                window.x(), window.y(), window.width(), window.height());
    minimize->click();
    settle_until([&] { return window.isMinimized(); });
    ok &= window.isMinimized();
    std::printf("[cpp] window minimized=%d\n", int(window.isMinimized()));
    window.showNormal();
    settle_until([&] { return !window.isMinimized() && window.geometry() == normal; });
    // Menu visibility must leave window controls accessible.
    QAction *toggle = nullptr;
    for (auto *action : window.menuBar()->findChildren<QAction *>()) {
        if (action->property("tid").toByteArray() == "view:toggle-menu-bar") toggle = action;
    }
    if (!toggle) { std::printf("[cpp] menu visibility action missing\n"); return false; }
    toggle->trigger();
    settle();
    ok &= minimize->isVisible() && maximize->isVisible() && close->isVisible();
    std::printf("[cpp] window controls after hide=%d/%d/%d\n", int(minimize->isVisible()), int(maximize->isVisible()), int(close->isVisible()));
    toggle->trigger();
    settle();
    return ok;
}
bool address_login_and_new_form(MainWindow &window) {
    window.activateWindow();
    window.raise();
    settle_until([&] { return window.isActiveWindow(); });
    const int count = window.page_count();
    const int original = window.current_page();
    auto *banner = window.page_banner(original);
    auto *address = banner->findChild<QLineEdit *>(QStringLiteral("sessionAddress"));
    if (!address) return false;
    banner->edit_address();
    settle_until([&] { return address->hasFocus(); });
    bool ok = address->hasFocus() && address->selectedText() == banner->breadcrumb();
    std::printf("[cpp] address focus=%d selected=%d\n", int(address->hasFocus()),
        int(address->selectedText() == banner->breadcrumb()));
    QKeyEvent escape(QEvent::KeyPress, Qt::Key_Escape, Qt::NoModifier);
    QApplication::sendEvent(address, &escape);
    ok &= !address->isVisible() && window.page_count() == count;
    std::printf("[cpp] address Escape hidden=%d count=%d/%d\n", int(!address->isVisible()), window.page_count(), count);
    banner->edit_address();
#ifdef Q_OS_WIN
    const QString shell = QStringLiteral("cmd");
#else
    const QString shell = QStringLiteral("bash");
#endif
    address->setText(QStringLiteral("shell://") + shell);
    QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
    QApplication::sendEvent(address, &enter);
    settle();
    ok &= window.page_count() == count + 1 &&
        window.page_banner(window.current_page())->breadcrumb() == QStringLiteral("shell://") + shell;
    std::printf("[cpp] address shell opened=%d title=%s\n", int(window.page_count() == count + 1),
        qPrintable(window.q_title(window.page_id(window.current_page()))));
    if (window.page_count() > count) window.close_tab(window.current_page());
    window.focus_session(window.page_id(original));
    banner->edit_address();
    address->setText(QStringLiteral("127.0.0.1:1"));
    bool saw_login = false;
    bool password_step = false;
    QTimer::singleShot(25, &window, [&] {
        auto *dialog = window.findChild<QDialog *>(QStringLiteral("sshLogin"));
        if (!dialog) return;
        auto *user = dialog->findChild<QLineEdit *>(QStringLiteral("loginUser"));
        if (user) settle_until([&] { return user->hasFocus(); });
        saw_login = dialog->isVisible() && !dialog->isWindow() && user && user->hasFocus();
        saw_login &= address->isVisible() && address->isReadOnly() && address->text() == QStringLiteral("127.0.0.1:1");
        std::printf("[cpp] login dialog=%s user=%d focused=%d\n", qPrintable(dialog->objectName()), int(user != nullptr), int(user && user->hasFocus()));
        dialog->grab().save(QStringLiteral("native/target/selftests/ssh-login.png"));
        dialog->parentWidget()->grab().save(QStringLiteral("native/target/selftests/ssh-login-group.png"));
        auto *group = dialog->parentWidget();
        const QSize previous = group->size();
        group->resize(300, 240);
        saw_login &= group->rect().contains(dialog->geometry());
        std::printf("[cpp] login compact bounds=%d size=%dx%d group=%dx%d\n",
            int(group->rect().contains(dialog->geometry())), dialog->width(), dialog->height(), group->width(), group->height());
        group->resize(previous);
        if (user) {
            user->setText(QStringLiteral("test-user"));
            QKeyEvent next(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
            QApplication::sendEvent(user, &next);
            auto *password = dialog->findChild<QLineEdit *>(QStringLiteral("loginPassword"));
            password_step = password && password->echoMode() == QLineEdit::Password && password->text().isEmpty();
            std::printf("[cpp] login password step=%d\n", int(password_step));
        }
        dialog->reject();
    });
    QApplication::sendEvent(address, &enter);
    ok &= saw_login && password_step && window.page_count() == count;
    std::printf("[cpp] login cancel count=%d/%d\n", window.page_count(), count);
    bool saw_form = false;
    QTimer::singleShot(25, &window, [&] {
        auto *form = dynamic_cast<SessionForm *>(QApplication::activeModalWidget());
        saw_form = form && form->protocol_pages() == 5;
        if (form) {
            bool shell_visible = false;
            for (auto *choice : form->findChildren<QComboBox *>()) {
                if (!choice->isVisible() || choice->visibleRegion().isEmpty() ||
                    !choice->isEditable() || choice->findText(QStringLiteral("auto")) < 0) continue;
                choice->setCurrentText(shell);
                SessCfg draft;
                form->collect(&draft);
                shell_visible = QString::fromUtf8(draft.local_shell.data(), int(draft.local_shell.size())) == shell;
            }
            saw_form &= shell_visible;
            std::printf("[cpp] form Shell chooser visible=%d\n", int(shell_visible));
            form->grab().save(QStringLiteral("native/target/selftests/new-session.png"));
        }
        if (auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget())) dialog->reject();
    });
    auto *action = chrome::find_menu_action(window.menuBar(), QStringLiteral("session:new"));
    if (action) action->trigger();
    ok &= action && saw_form && window.page_count() == count;
    std::printf("[cpp] full form action=%d shown=%d count=%d/%d\n", int(action != nullptr), int(saw_form), window.page_count(), count);
    return ok;
}
} // namespace

bool check_chrome_layout(MainWindow &window) {
    const bool notices = notification_bounds(window);
    const bool scrolling = file_scroll_interaction();
    const bool actions = window_actions(window);
    const bool login = address_login_and_new_form(window);
    std::printf("[cpp] C9 long notification bounds: %s\n", notices ? "PASS" : "FAIL");
    std::printf("[cpp] C9 file scroll idle / wheel / drag / expiry: %s\n", scrolling ? "PASS" : "FAIL");
    std::printf("[cpp] C9 menu-row window controls: %s\n", actions ? "PASS" : "FAIL");
    std::printf("[cpp] C9 address Enter / login password step / cancel / full form: %s\n", login ? "PASS" : "FAIL");
    return notices && scrolling && actions && login;
}

bool check_tab_status_layout(MainWindow &window) {
    chrome::sync_tab_buttons(&window);
    QTabBar *bar = window.tabs_bar();
    QWidget *lead = bar->tabButton(0, QTabBar::LeftSide);
    auto *square = lead ? lead->findChild<QLabel *>(QStringLiteral("tabActivitySquare")) : nullptr;
    QWidget *tail = bar->tabButton(0, QTabBar::RightSide);
    auto *close = tail ? tail->findChild<QToolButton *>(QStringLiteral("tabClose")) : nullptr;
    const bool ok = square && close &&
                    square->x() == chrome::metrics::kTabLeadInset &&
                    tail->width() - close->width() == chrome::metrics::kTabCloseInset &&
                    bar->tabIcon(0).isNull() &&
                    bar->tabText(0) == MainWindow::q_title(window.page_id(0)) &&
                    !bar->tabToolTip(0).isEmpty();
    std::printf("[cpp] C9-j 单状态方块与完整标签名: %s\n", ok ? "通过" : "失败");
    return ok;
}

bool check_two_tab_alignment(MainWindow &window) {
    QApplication::processEvents();
    QTabBar *bar = window.tabs_bar();
    bool ok = bar->count() == 2 &&
                    bar->tabRect(0).top() == bar->tabRect(1).top() &&
                    bar->tabRect(0).height() == bar->tabRect(1).height() &&
                    bar->tabText(0) == MainWindow::q_title(window.page_id(0)) &&
                    bar->tabText(1) == MainWindow::q_title(window.page_id(1));
    auto *crumb = window.page_banner(0)->findChild<QWidget *>(QStringLiteral("sessionCrumb"));
    const auto crumbs = crumb ? crumb->findChildren<QToolButton *>() : QList<QToolButton *>();
    const bool crumb_ok = crumbs.size() == 3 && crumbs[0]->text() == QStringLiteral("Shell") &&
        crumbs[1]->text() == i18n::tr("Shell 会话") && crumbs[2]->text() == MainWindow::q_title(window.page_id(0));
    ok &= crumb_ok;
    const QImage header = window.page_banner(0)->grab().toImage();
    const bool flat = header.pixelColor(header.width() / 2, header.height() - 1) ==
        header.pixelColor(header.width() / 2, header.height() - 2);
    ok &= flat;
    std::printf("[cpp] C9-p6 flat connection header bottom edge: %s\n", flat ? "PASS" : "FAIL");
    std::printf("[cpp] C9-p5 breadcrumb display names: %s\n", crumb_ok ? "PASS" : "FAIL");
    chrome::sync_tab_buttons(&window);
    settle();
    ok &= painted_tab_alignment(bar);
    ok &= painted_group_alignment(bar);
    std::printf("[cpp] C9-p2 普通双标签无编号且图标/关闭钮居中: %s\n", ok ? "通过" : "失败");
    // Same saved session opened in another window gets a distinct instance label.
    // Equal shell names alone must not be interpreted as copies.
    MainWindow first(false), second(false);
    SessCfg cfg = sess_active_config(window.page_id(0));
    cfg.id = rust::String("tab-label-selftest");
    cfg.name = rust::String("build gyp");
    const uint32_t a = sess_open_draft(cfg), b = sess_open_draft(cfg);
    first.add_tab_with(a);
    first.refresh_chrome();
    bool instances = first.tabs_bar()->tabText(0) == MainWindow::q_title(a);
    second.add_tab_with(b);
    second.refresh_chrome();
    instances &= first.tabs_bar()->tabText(0) == MainWindow::q_title(a) + QStringLiteral(" (1)") &&
                 second.tabs_bar()->tabText(0) == MainWindow::q_title(b) + QStringLiteral(" (2)");
    first.add_tab();
    first.tabs_bar()->moveTab(0, 1);
    first.refresh_chrome();
    instances &= first.tabs_bar()->tabText(1) == MainWindow::q_title(a) + QStringLiteral(" (1)");
    second.close_tab(0);
    first.refresh_chrome();
    instances &= first.tabs_bar()->tabText(1) == MainWindow::q_title(a);
    first.show(); settle(); chrome::sync_tab_buttons(&first); settle();
    const bool descenders = painted_tab_alignment(first.tabs_bar());
    cfg.id = rust::String("tab-glyph-cjk"); cfg.name = rust::String("中文终端");
    second.add_tab_with(sess_open_draft(cfg));
    second.show(); settle(); chrome::sync_tab_buttons(&second); settle();
    const bool cjk = painted_tab_alignment(second.tabs_bar());
    std::printf("[cpp] C9-p4 可见字形对齐（含下降字母及中文）: %s\n", descenders && cjk ? "通过" : "失败");
    std::printf("[cpp] C9-p3 同一会话跨窗口编号/拖动不改号/单实例还原: %s\n", instances ? "通过" : "失败");
    ok &= instances && descenders && cjk;
    return ok;
}
