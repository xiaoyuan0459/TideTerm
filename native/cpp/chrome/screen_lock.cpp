#include "cpp/chrome/screen_lock.h"
#include "cpp/chrome/screen_lock_auth.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QCloseEvent>
#include <QDialog>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QPointer>
#include <QPushButton>
#include <QShowEvent>
#include <QVBoxLayout>

namespace chrome {
namespace {

// A window-sized application modal blocks menus and floated panels as well as the PTY.
// An unset optional password keeps the user-selected Enter-to-unlock WindTerm mode.
class ScreenLock final : public QDialog {
public:
    explicit ScreenLock(MainWindow *window) : QDialog(window), window_(window) {
        setObjectName(QStringLiteral("screenLock"));
        setWindowTitle(i18n::tr("屏幕锁定"));
        setWindowFlags(Qt::Dialog | Qt::FramelessWindowHint);
        setWindowModality(Qt::ApplicationModal);
        if (!window->isActiveWindow()) setAttribute(Qt::WA_ShowWithoutActivating);
        setAutoFillBackground(true);
        QPalette colors = palette();
        colors.setColor(QPalette::Window, ui_colors().term_bg);
        colors.setColor(QPalette::WindowText, ui_colors().term_fg);
        setPalette(colors);
        auto *layout = new QVBoxLayout(this);
        layout->setAlignment(Qt::AlignCenter);
        unlock_page_ = new QWidget(this);
        unlock_page_->setFixedWidth(440);
        layout->addWidget(unlock_page_, 0, Qt::AlignCenter);
        make_unlock_page();
        window_->installEventFilter(this);
        qApp->installEventFilter(this);
    }
    void reject() override {}

protected:
    void showEvent(QShowEvent *event) override {
        QDialog::showEvent(event);
        setGeometry(window_->geometry());
    }
    void closeEvent(QCloseEvent *event) override { event->ignore(); }
    void keyPressEvent(QKeyEvent *event) override {
        if ((event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) &&
            !(event->modifiers() & (Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier))) {
            attempt_unlock();
        }
        event->accept(); // Enter is consumed here, never forwarded to the active terminal.
    }
    bool eventFilter(QObject *watched, QEvent *event) override {
        if (event->type() == QEvent::Shortcut) return true;
        if (watched == window_ && (event->type() == QEvent::Move || event->type() == QEvent::Resize)) {
            setGeometry(window_->geometry());
        }
        return QDialog::eventFilter(watched, event);
    }

private:
    void make_unlock_page() {
        password_ = nullptr;
        error_ = nullptr;
        auto *layout = new QVBoxLayout(unlock_page_);
        layout->setSpacing(18);
        auto *icon = new QLabel(unlock_page_);
        icon->setPixmap(scalable_icon(QStringLiteral("lock"), 40, ui_colors().accent).pixmap(40));
        icon->setAlignment(Qt::AlignCenter);
        layout->addWidget(icon);
        auto *title = new QLabel(i18n::tr("屏幕锁定"), unlock_page_);
        title->setAlignment(Qt::AlignCenter);
        layout->addWidget(title);
        if (prefs_lock_password_is_set()) {
            password_ = new QLineEdit(unlock_page_);
            password_->setObjectName(QStringLiteral("screenLockPassword"));
            password_->setEchoMode(QLineEdit::Password);
            password_->setMaxLength(128);
            password_->setPlaceholderText(i18n::tr("请输入锁屏密码"));
            password_->setAccessibleName(password_->placeholderText());
            password_->setFixedWidth(240);
            layout->addWidget(password_, 0, Qt::AlignHCenter);
            error_ = new QLabel(unlock_page_);
            error_->setAlignment(Qt::AlignCenter);
            layout->addWidget(error_);
            password_->setFocus();
        }
        unlock_ = new QPushButton(password_ ? i18n::tr("解锁") : i18n::tr("按 Enter 解锁"), unlock_page_);
        unlock_->setAccessibleName(unlock_->text());
        unlock_->setFlat(true);
        unlock_->setDefault(true);
        layout->addWidget(unlock_);
        connect(unlock_, &QPushButton::clicked, this, [this] { attempt_unlock(); });
        const uint32_t minutes = prefs_lock_idle_minutes();
        auto *idle = new QLabel(minutes == 0 ? i18n::tr("自动锁屏已关闭") :
            i18n::tr("无操作 %1 分钟后自动锁屏").arg(minutes), unlock_page_);
        idle->setAlignment(Qt::AlignCenter);
        layout->addWidget(idle);
    }
    void attempt_unlock() {
        if (!password_) { accept(); return; }
        if (!unlock_->isEnabled()) return;
        const QString password = password_->text();
        if (password.isEmpty()) { error_->setText(i18n::tr("请输入锁屏密码")); return; }
        password_->setEnabled(false);
        unlock_->setEnabled(false);
        error_->setText(i18n::tr("正在验证…"));
        run_lock_auth(this, [password] {
            const rust::String error = prefs_lock_password_verify(rust::String(password.toUtf8().toStdString()));
            return QString::fromUtf8(error.data(), int(error.size()));
        }, [this](const QString &error) {
            password_->clear();
            password_->setEnabled(true);
            unlock_->setEnabled(true);
            if (error.isEmpty()) accept();
            else { error_->setText(i18n::tr(error)); password_->setFocus(); }
        });
    }
    MainWindow *window_;
    QLineEdit *password_ = nullptr;
    QLabel *error_ = nullptr;
    QPushButton *unlock_ = nullptr;
    QWidget *unlock_page_ = nullptr;
};

} // namespace

void lock_screen(MainWindow *window) {
    if (!window || window->property("screenLocked").toBool()) return;
    QPointer<QWidget> previous_focus = QApplication::focusWidget();
    QList<QPointer<QWidget>> hidden;
    // QObject ownership crosses floating-window boundaries; QWidget ancestry does not.
    for (auto *widget : window->findChildren<QWidget *>()) {
        if (widget->isWindow() && widget->isVisible()) hidden.append(widget);
    }
    window->setProperty("screenLocked", true);
    term_set_locked(true);
    {
        ScreenLock lock(window);
        for (auto widget : hidden) if (widget) widget->hide();
        lock.exec();
    }
    term_set_locked(false);
    window->setProperty("screenLocked", false);
    prefs_lock_activity();
    for (auto widget : hidden) if (widget) widget->show();
    window->activateWindow();
    if (previous_focus && previous_focus->isVisible()) previous_focus->setFocus();
}

} // namespace chrome
