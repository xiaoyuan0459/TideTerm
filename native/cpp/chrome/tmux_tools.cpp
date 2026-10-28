#include "cpp/chrome/history_tools.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QLabel>
#include <QLineEdit>
#include <QPointer>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
class TmuxTools final : public QDialog {
public:
    explicit TmuxTools(PtyView *view) : QDialog(view), view_(view), source_(view->tab()),
        generation_(term_history_state(source_).generation) {
        setAttribute(Qt::WA_DeleteOnClose); setObjectName(QStringLiteral("tmuxTools"));
        setWindowTitle(i18n::tr("tmux 会话")); resize(680, 360);
        auto *layout = new QVBoxLayout(this);
        auto *help = new QLabel(i18n::tr("仅向当前终端发送一次预览命令，不广播。需目标主机安装 tmux；服务端历史仍受 history-limit 限制。客户端分屏独立管理。"), this);
        help->setWordWrap(true); layout->addWidget(help);
        name_ = new QLineEdit(QStringLiteral("TideTerm"), this); name_->setObjectName(QStringLiteral("tmuxName"));
        name_->setMaxLength(64); layout->addWidget(new QLabel(i18n::tr("会话名（字母、数字、下划线、短横线）"), this)); layout->addWidget(name_);
        action_ = new QComboBox(this); action_->setObjectName(QStringLiteral("tmuxAction"));
        action_->addItems({i18n::tr("进入或重新附加"), i18n::tr("分离当前客户端"), i18n::tr("浏览服务端历史（less）")}); layout->addWidget(action_);
        command_ = new QLabel(this); command_->setTextFormat(Qt::PlainText); command_->setWordWrap(true);
        command_->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(command_);
        confirm_ = new QCheckBox(i18n::tr("我已确认当前为空的 POSIX Shell 提示符；浏览服务端历史还需 less 和 mktemp"), this);
        layout->addWidget(confirm_);
        status_ = new QLabel(this); status_->setTextFormat(Qt::PlainText); status_->setWordWrap(true); layout->addWidget(status_);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
        buttons->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
        auto *preview = buttons->addButton(i18n::tr("预览命令"), QDialogButtonBox::ActionRole);
        preview->setObjectName(QStringLiteral("tmuxPreview"));
        send_ = buttons->addButton(i18n::tr("向当前终端发送一次"), QDialogButtonBox::ActionRole);
        send_->setObjectName(QStringLiteral("tmuxSend")); send_->setEnabled(false); layout->addWidget(buttons);
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::close);
        connect(name_, &QLineEdit::textChanged, this, [this] { invalidate(); });
        connect(action_, &QComboBox::currentIndexChanged, this, [this] { invalidate(); });
        connect(confirm_, &QCheckBox::toggled, this, [this](bool checked) { send_->setEnabled(checked && token_ != 0); });
        connect(preview, &QPushButton::clicked, this, [this] {
            const auto value = tmux_prepare(source_, name_->text().toUtf8().toStdString(), uint8_t(action_->currentIndex()));
            token_ = value.token; command_->setText(q(value.command));
            status_->setText(q(value.error)); send_->setEnabled(token_ != 0 && confirm_->isChecked());
        });
        connect(send_, &QPushButton::clicked, this, [this] {
            const auto error = tmux_send(source_, token_, confirm_->isChecked());
            token_ = 0; send_->setEnabled(false);
            status_->setText(error.empty() ? i18n::tr("已排队，请查看终端结果；失败可重新预览") : i18n::tr("操作失败：%1").arg(q(error)));
        });
        auto *timer = new QTimer(this); timer->setInterval(150);
        connect(timer, &QTimer::timeout, this, [this] {
            const auto state = term_history_state(source_);
            if (!view_ || view_->is_closed() || !state.available || state.generation != generation_) close();
        }); timer->start();
    }
    ~TmuxTools() override { tmux_cancel(source_); }
private:
    void invalidate() { tmux_cancel(source_); token_ = 0; send_->setEnabled(false); command_->clear(); status_->clear(); }
    QPointer<PtyView> view_;
    uint32_t source_, generation_;
    uint64_t token_ = 0;
    QLineEdit *name_;
    QComboBox *action_;
    QLabel *command_, *status_;
    QCheckBox *confirm_;
    QPushButton *send_;
};
}
void chrome::show_tmux_tools(PtyView *view) {
    if (!view || view->is_closed() || !term_history_state(view->tab()).available) return;
    for (auto *child : view->findChildren<QDialog *>(QString(), Qt::FindDirectChildrenOnly)) {
        if (auto *existing = dynamic_cast<TmuxTools *>(child)) { existing->show(); existing->raise(); return; }
    }
    auto *dialog = new TmuxTools(view); dialog->show(); dialog->raise(); dialog->activateWindow();
}
