#include "cpp/chrome/login_dialog.h"
#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/main_window.h"
#include <QCheckBox>
#include <QAction>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QStackedWidget>
#include <QTabBar>
#include <QTimer>
#include <QVBoxLayout>
#include <QShowEvent>
#include <QEvent>
#include <QScrollArea>
#include <QStyle>
#include <QGridLayout>
#include <functional>
#include <QTabWidget>
#include <QSplitter>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
class HostedLogin final : public QDialog {
public:
    explicit HostedLogin(QWidget *parent) : QDialog(parent, Qt::SubWindow | Qt::FramelessWindowHint) {
        parent->installEventFilter(this);
    }
    std::function<void(int)> reflow_footer;
    std::function<void()> resize_backdrop;
protected:
    void showEvent(QShowEvent *event) override {
        center();
        QDialog::showEvent(event);
        raise();
    }
    bool eventFilter(QObject *watched, QEvent *event) override {
        if (watched == parentWidget() && event->type() == QEvent::Resize) center();
        return QDialog::eventFilter(watched, event);
    }
private:
    void center() {
        if (!parentWidget()) return;
        const int available = qMax(240, qMin(510, parentWidget()->width() - 24));
        if (reflow_footer) reflow_footer(available);
        if (layout()) layout()->activate();
        resize(available,
            qMax(150, qMin(sizeHint().height(), parentWidget()->height() - 24)));
        move(qMax(0, (parentWidget()->width() - width()) / 2),
            qMax(0, (parentWidget()->height() - height()) / 2));
        if (resize_backdrop) resize_backdrop();
    }
};
}

bool run_login_dialog(QWidget *parent, const QuickTarget &target, SessCfg *cfg) {
    QWidget *host_group = parent;
    if (auto *window = dynamic_cast<MainWindow *>(parent)) host_group = window->tabs_bar()->parentWidget();
    QWidget backdrop(host_group);
    backdrop.setAttribute(Qt::WA_StyledBackground, true);
    backdrop.setFocusPolicy(Qt::NoFocus);
    QWidget *terminal_area = nullptr;
    if (auto *group = qobject_cast<QTabWidget *>(host_group); group && group->currentWidget()) {
        terminal_area = group->currentWidget()->findChild<QSplitter *>();
        if (!terminal_area) terminal_area = group->currentWidget();
    }
    HostedLogin dialog(host_group);
    dialog.resize_backdrop = [&] {
        if (terminal_area) backdrop.setGeometry(QRect(terminal_area->mapTo(host_group, QPoint()), terminal_area->size()));
    };
    dialog.resize_backdrop();
    backdrop.setVisible(terminal_area != nullptr);
    dialog.setObjectName(QStringLiteral("sshLogin"));
    dialog.setWindowTitle(i18n::tr("登录"));
    auto *outer = new QVBoxLayout(&dialog);
    outer->setContentsMargins(12, 10, 12, 12);
    auto *scroll = new QScrollArea(&dialog);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setWidgetResizable(true);
    scroll->setMinimumSize(0, 64);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *body = new QWidget(scroll);
    auto *root = new QVBoxLayout(body);
    root->setContentsMargins(0, 0, 0, 0);
    root->setSpacing(10);
    scroll->setWidget(body);
    outer->addWidget(scroll, 1);
    auto *heading = new QLabel(i18n::tr("登录"), &dialog);
    heading->setObjectName(QStringLiteral("loginHeading"));
    heading->setAlignment(Qt::AlignCenter);
    root->addWidget(heading);
    auto *tabs = new QTabBar(&dialog);
    tabs->setObjectName(QStringLiteral("loginModes"));
    tabs->addTab(QStringLiteral("OneKey"));
    tabs->addTab(i18n::tr("账户"));
    tabs->setCurrentIndex(0);
    tabs->setExpanding(true);
    root->addWidget(tabs);
    root->addWidget(new QLabel(i18n::tr("主机:"), &dialog));
    auto *host = new QLabel(q(target.host), &dialog);
    host->setTextFormat(Qt::PlainText);
    host->setWordWrap(true);
    root->addWidget(host);
    root->addWidget(new QLabel(QStringLiteral("SHA-256:"), &dialog));
    auto *fingerprint = new QLabel(i18n::tr("验证中…"), &dialog);
    fingerprint->setObjectName(QStringLiteral("loginFingerprint"));
    fingerprint->setTextFormat(Qt::PlainText);
    fingerprint->setWordWrap(true);
    fingerprint->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto *trust_row = new QHBoxLayout;
    auto *trust = new QLabel(&dialog);
    trust->setObjectName(QStringLiteral("loginTrust"));
    trust->hide();
    trust_row->addWidget(trust, 0, Qt::AlignTop);
    trust_row->addWidget(fingerprint, 1);
    root->addLayout(trust_row);
    auto *fields = new QStackedWidget(&dialog);
    auto *accounts = new QComboBox(fields);
    accounts->setObjectName(QStringLiteral("loginOneKey"));
    for (const auto &account : onekey_rows())
        accounts->addItem(q(account.name) + QStringLiteral(" (%1)").arg(q(account.username)), q(account.id));
    auto *manual = new QWidget(fields);
    auto *form = new QVBoxLayout(manual);
    form->setContentsMargins(0, 0, 0, 0);
    auto *label = new QLabel(i18n::tr("用户:(&U)"), manual);
    auto *entry = new QLineEdit(q(target.user), manual);
    entry->setObjectName(QStringLiteral("loginUser"));
    entry->setAccessibleName(i18n::tr("用户名"));
    label->setBuddy(entry);
    auto *eye = entry->addAction(chrome::scalable_icon(QStringLiteral("view"), 14), QLineEdit::TrailingPosition);
    eye->setToolTip(i18n::tr("显示/隐藏密码"));
    eye->setVisible(false);
    QObject::connect(eye, &QAction::triggered, entry, [entry, eye] {
        entry->setEchoMode(entry->echoMode() == QLineEdit::Password ? QLineEdit::Normal : QLineEdit::Password);
        const QString name = entry->echoMode() == QLineEdit::Password ? QStringLiteral("view") : QStringLiteral("view_off");
        eye->setProperty("chromeIcon", name);
        eye->setIcon(chrome::scalable_icon(name, 14));
    });
    form->addWidget(label);
    form->addWidget(entry);
    fields->addWidget(manual);
    fields->addWidget(accounts);
    fields->setCurrentIndex(0);
    root->addWidget(fields);
    auto *message = new QLabel(&dialog);
    message->setObjectName(QStringLiteral("loginError"));
    message->setTextFormat(Qt::PlainText);
    message->setWordWrap(true);
    root->addWidget(message);
    auto *footer = new QGridLayout;
    auto *remember = new QCheckBox(i18n::tr("下次自动登录(&A)"), &dialog);
    footer->setColumnStretch(1, 1);
    auto *buttons = new QDialogButtonBox(&dialog);
    auto *next = buttons->addButton(i18n::tr("继续"), QDialogButtonBox::AcceptRole);
    next->setMinimumWidth(80);
    next->setFixedHeight(24);
    next->setDefault(true);
    auto *cancel = buttons->addButton(i18n::tr("取消"), QDialogButtonBox::RejectRole);
    cancel->setMinimumWidth(80);
    cancel->setFixedHeight(24);
    dialog.reflow_footer = [=](int width) {
        footer->removeWidget(remember);
        footer->removeWidget(buttons);
        if (width < 420) {
            footer->addWidget(remember, 0, 0, 1, 3);
            footer->addWidget(buttons, 1, 2);
        } else {
            footer->addWidget(remember, 0, 0);
            footer->addWidget(buttons, 0, 2);
        }
    };
    dialog.reflow_footer(qMin(510, host_group->width() - 24));
    outer->addLayout(footer);
    QString username;
    bool password_step = false;
    bool key_changed = false;
    QObject::connect(tabs, &QTabBar::currentChanged, fields, [=, &key_changed](int index) {
        fields->setCurrentIndex(index);
        next->setEnabled(!key_changed && (index == 0 || accounts->count() > 0));
        remember->setEnabled(index == 0);
        if (index == 0) entry->setFocus();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] {
        if (key_changed) return;
        if (tabs->currentIndex() == 1) {
            if (accounts->currentIndex() < 0) return;
            cfg->ssh_auth = 3;
            cfg->ssh_account_id = rs(accounts->currentData().toString());
            dialog.accept();
        } else if (!password_step) {
            username = entry->text().trimmed();
            if (username.isEmpty()) {
                message->setText(i18n::tr("请输入用户名"));
                entry->setFocus();
                return;
            }
            password_step = true;
            message->clear();
            label->setText(i18n::tr("密码:(&P)"));
            entry->clear();
            entry->setEchoMode(QLineEdit::Password);
            entry->setObjectName(QStringLiteral("loginPassword"));
            entry->setAccessibleName(i18n::tr("密码"));
            eye->setVisible(true);
            entry->setFocus();
        } else {
            cfg->ssh_user = rs(username);
            cfg->ssh_password = rs(entry->text());
            cfg->ssh_remember = remember->isChecked();
            dialog.accept();
        }
    });
    // The preview is read-only; authentication rechecks the host key on its connection.
    const uint64_t request = context_inspect_host(target.host, target.port ? target.port : 22);
    QTimer poll;
    QObject::connect(&poll, &QTimer::timeout, &dialog, [&] {
        const auto info = context_take_host_preview(request);
        if (!info.ready) return;
        poll.stop();
        if (!info.error.empty()) {
            fingerprint->setText(i18n::tr("未验证"));
            message->setText(q(info.error));
            return;
        }
        host->setText(q(target.host) + QStringLiteral(" [%1]").arg(q(info.banner)));
        const QString state = info.state == 1 ? i18n::tr("已知")
            : info.state == 3 ? i18n::tr("密钥变化") : i18n::tr("未知");
        trust->setText(state);
        trust->setProperty("hostState", info.state == 1 ? "known" : info.state == 3 ? "changed" : "unknown");
        trust->style()->unpolish(trust);
        trust->style()->polish(trust);
        trust->show();
        fingerprint->setText(QStringLiteral("[%1]").arg(q(info.fingerprint)));
        key_changed = info.state == 3;
        if (key_changed) {
            next->setEnabled(false);
            message->setText(i18n::tr("主机密钥已改变，请核实后再连接"));
        }
    });
    poll.start(50);
    entry->setFocus();
    QTimer::singleShot(0, &dialog, [scroll, entry] { scroll->ensureWidgetVisible(entry); entry->setFocus(); });
    const bool accepted = dialog.exec() == QDialog::Accepted;
    poll.stop();
    context_cancel_host_preview(request);
    entry->clear();
    if (!accepted) return false;
    cfg->name = target.host;
    if (cfg->ssh_remember && chrome::ensure_vault_ready(parent)) {
        const auto id = sess_save(*cfg);
        if (!id.empty()) {
            cfg->id = id;
            chrome::sync_saved_credentials(*cfg, id);
        }
    }
    return true;
}
