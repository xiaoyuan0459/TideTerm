// Menu-level regression: a passwordless lock covers content and consumes its unlock key.
#include "cpp/chrome/selftest_screen_lock.h"
#include "cpp/chrome/screen_lock.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QApplication>
#include <QDialog>
#include <QDockWidget>
#include <QDir>
#include <QInputDialog>
#include <QInputMethodEvent>
#include <QElapsedTimer>
#include <QLineEdit>
#include <QKeyEvent>
#include <QMenuBar>
#include <QPointer>
#include <QPixmap>
#include <QPushButton>
#include <QSpinBox>
#include <QThread>
#include <QTimer>
#include <cstdio>

namespace {
bool wait_until(const std::function<bool()> &ready, int timeout = 5000) {
    QElapsedTimer clock;
    clock.start();
    while (!ready() && clock.elapsed() < timeout) {
        QApplication::processEvents();
        QThread::msleep(5);
    }
    return ready();
}

template<class T> T *visible(QWidget *root, const char *name) {
    for (auto *widget : root->findChildren<T *>(QLatin1String(name)))
        if (widget->isVisible()) return widget;
    return nullptr;
}

bool click(QWidget *root, const char *label) {
    for (auto *button : root->findChildren<QPushButton *>()) {
        if (button->isVisible() && button->text() == i18n::tr(label)) { button->click(); return true; }
    }
    return false;
}

bool check_password_lock(MainWindow &window, QAction *action) {
    const QString password = QStringLiteral("fixture-screen-lock-password");
    auto *settings = chrome::find_menu_action(window.menuBar(), QStringLiteral("mode:lock-settings"));
    if (!settings || settings->parent() != action->parent()) return false;
    const auto edit = [&](const std::function<bool(QDialog *)> &operation) {
        bool result = false;
        QObject guard;
        QTimer::singleShot(50, &guard, [&] {
            auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
            if (!dialog) return;
            if (dialog->objectName() == QStringLiteral("screenLockSettingsDialog")) result = operation(dialog);
            if (dialog->isVisible()) dialog->done(QDialog::Rejected);
        });
        settings->trigger();
        return result;
    };
    bool ok = edit([&](QDialog *dialog) {
        auto *next = visible<QLineEdit>(dialog, "screenLockNext");
        auto *confirm = visible<QLineEdit>(dialog, "screenLockConfirm");
        auto *minutes = visible<QSpinBox>(dialog, "screenLockIdleMinutes");
        if (!next || !confirm || !minutes) return false;
        bool checked = minutes->value() == 0 && !window.property("screenLocked").toBool();
        next->setText(password);
        click(dialog, "保存");
        checked &= dialog->isVisible() && !prefs_lock_password_is_set();
        click(dialog, "取消");
        return checked && !dialog->isVisible() && !prefs_lock_password_is_set();
    });
    ok &= edit([&](QDialog *dialog) {
        auto *next = visible<QLineEdit>(dialog, "screenLockNext");
        auto *confirm = visible<QLineEdit>(dialog, "screenLockConfirm");
        if (!next || !confirm) return false;
        dialog->grab().save(QCoreApplication::applicationDirPath() +
            QStringLiteral("/../diagnostics/ui-format-lock-settings/screen-lock-settings-%1-%2.png").arg(QGuiApplication::platformName(), i18n::tr("保存")));
        next->setText(password);
        confirm->setText(password);
        click(dialog, "保存");
        dialog->reject(); dialog->close();
        const bool busy = dialog->isVisible();
        const bool finished = wait_until([dialog] { return !dialog->isVisible(); });
        return busy && finished && prefs_lock_password_is_set() && !window.property("screenLocked").toBool();
    });
    MainWindow other(false);
    auto *other_settings = chrome::find_menu_action(other.menuBar(), QStringLiteral("mode:lock-settings"));
    bool protected_lock = false;
    QTimer::singleShot(50, &window, [&] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog) return;
        protected_lock = window.property("screenLocked").toBool() &&
            !dialog->findChild<QWidget *>(QStringLiteral("screenLockSettings")) &&
            !dialog->findChild<QPushButton *>(QStringLiteral("screenLockSettingsButton"));
        settings->trigger();
        if (other_settings) other_settings->trigger();
        protected_lock &= other_settings && QApplication::activeModalWidget() == dialog && prefs_lock_password_is_set();
        auto *input = visible<QLineEdit>(dialog, "screenLockPassword");
        if (input) {
            QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
            QApplication::sendEvent(dialog, &enter);
            protected_lock &= dialog->isVisible(); // Empty Enter must no longer unlock.
            input->setText(QStringLiteral("wrong"));
            QApplication::sendEvent(dialog, &enter);
            protected_lock &= wait_until([input] { return input->isEnabled(); }) && dialog->isVisible();
            dialog->grab().save(QCoreApplication::applicationDirPath() +
                QStringLiteral("/../diagnostics/ui-format-lock-settings/screen-lock-password.png"));
            input->setText(password);
            QApplication::sendEvent(dialog, &enter);
            protected_lock &= wait_until([dialog] { return !dialog->isVisible(); });
        }
        if (dialog->isVisible()) dialog->done(QDialog::Rejected);
    });
    action->trigger();
    ok &= protected_lock;
    ok &= edit([&](QDialog *dialog) {
        auto *current = visible<QLineEdit>(dialog, "screenLockCurrent");
        auto *minutes = visible<QSpinBox>(dialog, "screenLockIdleMinutes");
        if (!current || !minutes) return false;
        current->setText(QStringLiteral("wrong")); minutes->setValue(1);
        click(dialog, "保存");
        bool checked = wait_until([current] { return current->isEnabled(); }) &&
            dialog->isVisible() && prefs_lock_idle_minutes() == 0 && prefs_lock_password_is_set();
        click(dialog, "移除密码");
        checked &= wait_until([current] { return current->isEnabled(); }) && prefs_lock_password_is_set();
        current->setText(password); click(dialog, "保存");
        checked &= wait_until([dialog] { return !dialog->isVisible(); });
        return checked && prefs_lock_password_is_set() && prefs_lock_idle_minutes() == 1;
    });
    ok &= edit([&](QDialog *dialog) {
        auto *current = visible<QLineEdit>(dialog, "screenLockCurrent");
        if (!current) return false;
        current->setText(password); click(dialog, "移除密码");
        return wait_until([dialog] { return !dialog->isVisible(); }) && !prefs_lock_password_is_set();
    });
    return ok && !prefs_lock_password_is_set();
}

bool check_auto_lock(MainWindow &window) {
    if (prefs_lock_idle_minutes() != 1 || prefs_lock_password_is_set()) return false;
    auto *view = window.active_view();
    if (!view) return false;
    QInputMethodEvent command;
#ifdef Q_OS_WIN
    command.setCommitString(QStringLiteral("1..12 | ForEach-Object { Write-Output ('LOCK_' + 'TIMER_OUTPUT'); Start-Sleep -Seconds 6 }"));
#else
    command.setCommitString(QStringLiteral("for i in 1 2 3 4 5 6 7 8 9 10 11 12; do printf '%s%s\\n' 'LOCK_' 'TIMER_OUTPUT'; sleep 6; done"));
#endif
    QApplication::sendEvent(view, &command);
    QKeyEvent execute(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
    QApplication::sendEvent(view, &execute);
    QObject guard;
    auto *floating = window.docks()->files;
    const bool was_floating = floating->isFloating();
    const bool was_visible = floating->isVisible();
    floating->setFloating(true);
    floating->show();
    QElapsedTimer elapsed;
    elapsed.start();
    bool observed = false, not_early = false, unlocked = false, output_seen = false;
    qint64 triggered = 0;
    const auto sessions = window.pane_ids(0);
    QTimer::singleShot(10000, &guard, [&] {
        QKeyEvent activity(QEvent::KeyPress, Qt::Key_Shift, Qt::ShiftModifier);
        QApplication::sendEvent(floating, &activity);
    });
    QTimer::singleShot(65000, &guard, [&] { not_early = !window.property("screenLocked").toBool(); });
    QTimer::singleShot(55000, &guard, [&] {
        const auto text = term_screen_text(view->tab());
        output_seen = QString::fromUtf8(text.data(), int(text.size())).contains(QStringLiteral("LOCK_TIMER_OUTPUT"));
    });
    QTimer inspect;
    inspect.setInterval(100);
    QObject::connect(&inspect, &QTimer::timeout, &guard, [&] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != QStringLiteral("screenLock")) return;
        observed = window.property("screenLocked").toBool();
        triggered = elapsed.elapsed();
        dialog->grab().save(QCoreApplication::applicationDirPath() +
            QStringLiteral("/../diagnostics/ui-format-lock-settings/screen-lock-auto.png"));
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QApplication::sendEvent(dialog, &enter);
        unlocked = !dialog->isVisible();
        if (!unlocked) dialog->done(QDialog::Rejected);
    });
    QTimer::singleShot(80000, &guard, [&] {
        if (auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()))
            if (dialog->objectName() == QStringLiteral("screenLock")) dialog->done(QDialog::Rejected);
    });
    inspect.start();
    wait_until([&] { return observed; }, 82000);
    inspect.stop();
    floating->setFloating(was_floating);
    floating->setVisible(was_visible);
    const rust::String disabled = prefs_lock_settings_save(rust::String(), rust::String(), false, 0);
    const bool ok = observed && not_early && output_seen && triggered >= 69000 && unlocked &&
        !window.property("screenLocked").toBool() && window.pane_ids(0) == sessions && disabled.empty();
    std::printf("[cpp] C9 auto-lock real timer / activity reset / output / sessions: %s (at=%lldms)\n",
                ok ? "PASS" : "FAIL", static_cast<long long>(triggered));
    return ok;
}
}

bool check_screen_lock(MainWindow &window) {
    auto *action = chrome::find_menu_action(window.menuBar(), QStringLiteral("mode:lock-screen"));
    if (!action || !action->isEnabled()) return false;
    const auto sessions = window.pane_ids(0);
    auto *floating = window.docks()->files;
    const bool was_floating = floating->isFloating();
    const bool was_visible = floating->isVisible();
    floating->setFloating(true);
    floating->show();
    window.activateWindow();
    auto *view = window.active_view();
    if (view) view->setFocus();
    QApplication::processEvents();
    QPointer<QWidget> previous_focus = QApplication::focusWidget();
    bool observed = false;
    bool ok = false;
    QTimer::singleShot(50, &window, [&] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog) return;
        observed = true;
        auto *settings = chrome::find_menu_action(window.menuBar(), QStringLiteral("mode:lock-settings"));
        const bool no_settings = !dialog->findChild<QWidget *>(QStringLiteral("screenLockSettings")) &&
            !dialog->findChild<QPushButton *>(QStringLiteral("screenLockSettingsButton"));
        if (settings) settings->trigger();
        ok = no_settings && settings && !prefs_lock_password_is_set() && QApplication::activeModalWidget() == dialog;
        if (qobject_cast<QInputDialog *>(dialog)) {
            std::printf("[cpp] C9 lock FAIL: menu requested a password instead of locking\n");
            dialog->reject();
            return;
        }
        const bool covers_window = wait_until([&] { return dialog->geometry() == window.geometry(); }, 2000);
        ok &= covers_window && !floating->isVisible();
        if (!ok) std::printf("[cpp] C9 lock FAIL: initial cover=%d floating-hidden=%d window=%d,%d %dx%d dialog=%d,%d %dx%d\n",
                             covers_window, !floating->isVisible(), window.geometry().x(), window.geometry().y(), window.width(), window.height(),
                             dialog->geometry().x(), dialog->geometry().y(), dialog->width(), dialog->height());
        const QString evidence = QCoreApplication::applicationDirPath() + QStringLiteral("/../diagnostics/ui-format-lock-settings");
        QDir().mkpath(evidence);
        dialog->grab().save(evidence + QStringLiteral("/screen-lock.png"));
        QKeyEvent escape(QEvent::KeyPress, Qt::Key_Escape, Qt::NoModifier);
        QApplication::sendEvent(dialog, &escape);
        ok &= dialog->isVisible() && QApplication::activeModalWidget() == dialog;
        if (!dialog->isVisible() || QApplication::activeModalWidget() != dialog)
            std::printf("[cpp] C9 lock FAIL: Escape dismissed cover\n");
        dialog->close();
        ok &= dialog->isVisible();
        if (!dialog->isVisible()) std::printf("[cpp] C9 lock FAIL: close dismissed cover\n");
        window.resize(window.width() + 12, window.height() + 8);
        ok &= wait_until([&] { return dialog->geometry() == window.geometry(); }, 2000);
        if (dialog->geometry() != window.geometry()) std::printf("[cpp] C9 lock FAIL: resize left uncovered content\n");
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QApplication::sendEvent(dialog, &enter);
        ok &= !dialog->isVisible();
        if (dialog->isVisible()) std::printf("[cpp] C9 lock FAIL: Enter did not unlock\n");
        if (dialog->isVisible()) dialog->done(QDialog::Rejected);
    });
    action->trigger();
    QApplication::processEvents();
    ok &= observed && !QApplication::activeModalWidget() && floating->isVisible();
    ok &= window.pane_ids(0) == sessions;
    ok &= !previous_focus || QApplication::focusWidget() == previous_focus;
    if (QApplication::activeModalWidget() || !floating->isVisible() || window.pane_ids(0) != sessions ||
        (previous_focus && QApplication::focusWidget() != previous_focus))
        std::printf("[cpp] C9 lock FAIL: modal-cleared=%d floating-restored=%d sessions=%d focus-restored=%d\n",
                    !QApplication::activeModalWidget(), floating->isVisible(), window.pane_ids(0) == sessions,
                    !previous_focus || QApplication::focusWidget() == previous_focus);
    floating->setFloating(was_floating);
    floating->setVisible(was_visible);
    std::printf("[cpp] C9 passwordless screen lock / Enter / focus / floating panel: %s\n",
                ok ? "PASS" : "FAIL");
    const bool password_ok = check_password_lock(window, action);
    std::printf("[cpp] C9 menu settings / locked entry denied / old-password guard / correct unlock: %s\n",
                password_ok ? "PASS" : "FAIL");
    ok &= password_ok;
    if (password_ok) ok &= check_auto_lock(window);
    return ok;
}
