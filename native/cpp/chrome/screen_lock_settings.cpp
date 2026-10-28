#include "cpp/chrome/screen_lock_settings.h"
#include "cpp/chrome/screen_lock_auth.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include <QApplication>
#include <QCloseEvent>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QVBoxLayout>

namespace chrome {
namespace {
class LockSettings final : public QWidget {
public:
    LockSettings(QWidget *parent, std::function<void()> saved, std::function<void()> canceled)
        : QWidget(parent), saved_(std::move(saved)), canceled_(std::move(canceled)) {
        setObjectName(QStringLiteral("screenLockSettings"));
        setMaximumWidth(440);
        auto *layout = new QVBoxLayout(this);
        layout->addWidget(new QLabel(i18n::tr("锁屏设置"), this));
        auto *hint = new QLabel(i18n::tr("密码和自动锁屏时间保存到本机，重启后仍有效。"), this);
        hint->setWordWrap(true);
        layout->addWidget(hint);
        auto *form = new QFormLayout;
        const bool configured = prefs_lock_password_is_set();
        auto *state = new QLabel(configured ? i18n::tr("已设置密码，更改设置需要当前密码。")
                                           : i18n::tr("未设置密码，按 Enter 即可解锁。"), this);
        state->setWordWrap(true);
        layout->addWidget(state);
        layout->addLayout(form);
        idle_ = new QSpinBox(this);
        idle_->setObjectName(QStringLiteral("screenLockIdleMinutes"));
        idle_->setRange(0, 1440);
        idle_->setSpecialValueText(i18n::tr("不自动锁屏"));
        idle_->setSuffix(i18n::tr(" 分钟"));
        idle_->setValue(int(prefs_lock_idle_minutes()));
        idle_->setAccessibleName(i18n::tr("无操作自动锁屏"));
        form->addRow(i18n::tr("无操作自动锁屏"), idle_);
        current_ = entry("screenLockCurrent");
        if (configured) form->addRow(i18n::tr("当前密码"), current_);
        else current_->hide();
        next_ = entry("screenLockNext");
        next_->setPlaceholderText(i18n::tr("至少 8 个字符，留空保持现状"));
        form->addRow(i18n::tr("新密码（可选）"), next_);
        confirm_ = entry("screenLockConfirm");
        form->addRow(i18n::tr("确认新密码"), confirm_);
        auto *idle_hint = new QLabel(i18n::tr("0 表示关闭。终端输出不重置计时；锁屏期间会话继续运行。"), this);
        idle_hint->setWordWrap(true);
        layout->addWidget(idle_hint);
        error_ = new QLabel(this);
        error_->setWordWrap(true);
        layout->addWidget(error_);
        buttons_ = new QDialogButtonBox(this);
        auto *save = buttons_->addButton(i18n::tr("保存"), QDialogButtonBox::AcceptRole);
        save->setDefault(true);
        auto *remove = buttons_->addButton(i18n::tr("移除密码"), QDialogButtonBox::ActionRole);
        remove->setEnabled(configured);
        buttons_->addButton(i18n::tr("取消"), QDialogButtonBox::RejectRole);
        connect(save, &QPushButton::clicked, this, [this] { submit(false); });
        connect(remove, &QPushButton::clicked, this, [this] { submit(true); });
        connect(buttons_, &QDialogButtonBox::rejected, this, [this] { if (!pending_) canceled_(); });
        layout->addWidget(buttons_);
        (configured ? current_ : next_)->setFocus();
    }
    bool pending() const { return pending_; }
private:
    QLineEdit *entry(const char *name) {
        auto *line = new QLineEdit(this);
        line->setObjectName(QLatin1String(name));
        line->setEchoMode(QLineEdit::Password);
        line->setMaxLength(128);
        return line;
    }
    void submit(bool removing) {
        if (pending_) return;
        if (!removing && next_->text() != confirm_->text()) {
            error_->setText(i18n::tr("两次输入的新密码不一致"));
            return;
        }
        const QString old_password = current_->text();
        const QString new_password = removing ? QString() : next_->text();
        const uint32_t minutes = uint32_t(idle_->value());
        pending_ = true;
        buttons_->setEnabled(false);
        current_->setEnabled(false);
        next_->setEnabled(false);
        confirm_->setEnabled(false);
        idle_->setEnabled(false);
        error_->setText(i18n::tr("正在保存…"));
        run_lock_auth(this, [old_password, new_password, removing, minutes] {
            const rust::String error = prefs_lock_settings_save(
                rust::String(old_password.toUtf8().toStdString()),
                rust::String(new_password.toUtf8().toStdString()), removing, minutes);
            return QString::fromUtf8(error.data(), int(error.size()));
        }, [this](const QString &error) {
            pending_ = false;
            buttons_->setEnabled(true);
            current_->setEnabled(true);
            next_->setEnabled(true);
            confirm_->setEnabled(true);
            idle_->setEnabled(true);
            if (error.isEmpty()) { current_->clear(); next_->clear(); confirm_->clear(); saved_(); }
            else {
                error_->setText(i18n::tr(error));
                auto *input = current_->isVisible() ? current_ : next_;
                input->selectAll(); input->setFocus();
            }
        });
    }
    bool pending_ = false;
    QLineEdit *current_, *next_, *confirm_;
    QLabel *error_;
    QDialogButtonBox *buttons_;
    QSpinBox *idle_;
    std::function<void()> saved_, canceled_;
};
class SettingsDialog final : public QDialog {
public:
    explicit SettingsDialog(MainWindow *window) : QDialog(window) {
        setObjectName(QStringLiteral("screenLockSettingsDialog"));
        setWindowTitle(i18n::tr("锁屏设置"));
        setMinimumWidth(480);
        setWindowModality(Qt::ApplicationModal);
        auto *layout = new QVBoxLayout(this);
        form_ = new LockSettings(this, [this] { accept(); }, [this] { reject(); });
        layout->addWidget(form_);
    }
    void reject() override { if (!form_ || !form_->pending()) QDialog::reject(); }
protected:
    void closeEvent(QCloseEvent *event) override {
        if (form_ && form_->pending()) event->ignore();
        else QDialog::closeEvent(event);
    }
private:
    LockSettings *form_ = nullptr;
};
}

void show_lock_settings(MainWindow *window) {
    if (!window) return;
    for (auto *widget : QApplication::topLevelWidgets()) {
        auto *owner = dynamic_cast<MainWindow *>(widget);
        if (owner && owner->property("screenLocked").toBool()) {
            window->note(i18n::tr("请先解锁屏幕，再修改锁屏设置。"));
            return;
        }
    }
    prefs_lock_activity();
    SettingsDialog dialog(window);
    if (dialog.exec() == QDialog::Accepted) window->note(i18n::tr("锁屏设置已保存"));
}
}
