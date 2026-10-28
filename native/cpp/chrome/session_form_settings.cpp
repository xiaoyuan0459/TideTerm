#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_form_widgets.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/onekey_dialog.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QSet>
#include <QSpinBox>
#include <QStackedWidget>
#include <QToolButton>
#include <QTreeWidget>
#include <QVBoxLayout>
namespace {
QWidget *wrap(const QString &title, QWidget *body, QWidget *parent) {
    auto *w = new QWidget(parent);
    auto *lay = new QVBoxLayout(w);
    lay->setContentsMargins(0, 0, 0, 0);
    lay->addWidget(body);
    w->setWindowTitle(title);
    return w;
}
}
QFormLayout *SessionForm::build_settings() {
    // ─────────── 页 1：Shell > 进程 ───────────
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        shell_path_ = new QLineEdit(this);
        f->addRow(i18n::tr("自定义路径"), shell_path_);
        local_args_ = new QLineEdit(this);
        f->addRow(i18n::tr("参数"), local_args_);
        local_cwd_ = new QLineEdit(this);
        f->addRow(i18n::tr("工作目录"), local_cwd_);
        auto *page = new QWidget(settings_);
        chrome::prepare_session_fields(f);
        page->setLayout(f);
        settings_->addWidget(wrap(i18n::tr("进程"), page, settings_));
    }

    // ─────────── 页 2：SSH > 连接 ───────────
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        ssh_timeout_ = new chrome::SessionSpin(this);
        ssh_timeout_->setRange(1, 3600);
        ssh_timeout_->setSuffix(i18n::tr(" 秒"));
        ssh_timeout_->setValue(15);
        f->addRow(i18n::tr("连接超时:(T)"), ssh_timeout_);
        ssh_reconnect_ = new QCheckBox(i18n::tr("自动重连"), this);
        f->addRow(QString(), ssh_reconnect_);
        ssh_attempts_ = new chrome::SessionSpin(this);
        ssh_attempts_->setRange(0, 100);
        ssh_attempts_->setToolTip(i18n::tr("0 表示不重试"));
        f->addRow(i18n::tr("重试次数"), ssh_attempts_);
        ssh_interval_ = new chrome::SessionSpin(this);
        ssh_interval_->setRange(1, 3600);
        f->addRow(i18n::tr("重试间隔（秒）"), ssh_interval_);
        auto *page = new QWidget(settings_);
        chrome::prepare_session_fields(f);
        page->setLayout(f);
        settings_->addWidget(wrap(i18n::tr("连接设置"), page, settings_));
    }

    // ─────────── 页 4：串口 > 协议 ───────────
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        ser_baud_ = new chrome::SessionCombo(this);
        ser_baud_->setEditable(true);
        ser_baud_->addItems({QStringLiteral("9600"), QStringLiteral("19200"),
                             QStringLiteral("38400"), QStringLiteral("57600"),
                             QStringLiteral("115200"), QStringLiteral("230400"),
                             QStringLiteral("460800"), QStringLiteral("921600")});
        ser_baud_->setCurrentText(QStringLiteral("115200"));
        f->addRow(i18n::tr("波特率:(B)"), ser_baud_);
        ser_bits_ = new chrome::SessionCombo(this);
        ser_bits_->addItems({QStringLiteral("5"), QStringLiteral("6"),
                             QStringLiteral("7"), QStringLiteral("8")});
        ser_bits_->setCurrentText(QStringLiteral("8"));
        f->addRow(i18n::tr("数据位"), ser_bits_);
        ser_stop_ = new chrome::SessionCombo(this);
        ser_stop_->addItems({QStringLiteral("1"), QStringLiteral("2")});
        f->addRow(i18n::tr("停止位"), ser_stop_);
        ser_parity_ = new chrome::SessionCombo(this);
        ser_parity_->addItems({i18n::tr("无"), i18n::tr("奇"),
                               i18n::tr("偶")});
        f->addRow(i18n::tr("校验"), ser_parity_);
        ser_flow_ = new chrome::SessionCombo(this);
        ser_flow_->addItems({i18n::tr("无"), i18n::tr("硬件"),
                             QStringLiteral("XON/XOFF")});
        f->addRow(i18n::tr("流控"), ser_flow_);
        auto *page = new QWidget(settings_);
        chrome::prepare_session_fields(f);
        page->setLayout(f);
        settings_->addWidget(wrap(i18n::tr("协议"), page, settings_));
    }

    // ─────────── 页 3：SSH > 验证 ───────────
    QFormLayout *auth_form = nullptr;
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        auth_form = f;
        ssh_user_ = new QLineEdit(this);
        f->addRow(i18n::tr("用户名:(U)"), ssh_user_);
        ssh_auth_ = new chrome::SessionCombo(this);
        ssh_auth_->addItems({i18n::tr("密码"), i18n::tr("密钥"),
                             i18n::tr("键盘交互"), QStringLiteral("OneKey"), i18n::tr("SSH Agent")});
        f->addRow(i18n::tr("认证方式:(A)"), ssh_auth_);
        ssh_password_ = new QLineEdit(this);
        ssh_password_->setEchoMode(QLineEdit::Password);
        f->addRow(i18n::tr("密码:(P)"), ssh_password_);
        ssh_key_ = new QLineEdit(this);
        f->addRow(i18n::tr("私钥路径:(K)"), ssh_key_);
        ssh_passphrase_ = new QLineEdit(this);
        ssh_passphrase_->setEchoMode(QLineEdit::Password);
        f->addRow(i18n::tr("密钥口令"), ssh_passphrase_);
        ssh_remember_ = new QCheckBox(i18n::tr("记住密码（主密码加密保存）"), this);
        ssh_remember_->setToolTip(
            i18n::tr("密码以主密码派生密钥（AES-256-GCM）加密存入 vault.json，"
                     "下次连接自动填充；sessions.json 依旧不保存任何凭据。"));
        f->addRow(QString(), ssh_remember_);
        auto *page = new QWidget(settings_);
        chrome::prepare_session_fields(f);
        page->setLayout(f);
        settings_->addWidget(wrap(i18n::tr("验证"), page, settings_));
    }

    return auth_form;
}

void SessionForm::select_protocol_defaults(int kind) {
    choices_[choice_kind_] = {term_->currentText(), system_->currentText(), charset_->currentText(), collect_advanced(),
                             sftp_->isChecked(), sysmon_->isChecked(), true};
    auto &choice = choices_[kind];
    if (!choice.visited) {
        const auto cfg = sess_defaults_get(uint8_t(kind));
        auto text = [](const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); };
        choice = {text(cfg.terminal_emulation), text(cfg.system_shell), text(cfg.charset), cfg.options,
                  cfg.sftp_enabled, cfg.sysmon_enabled, true};
        if (kind == 0) {
            shell_->setCurrentText(cfg.local_shell.empty() ? QStringLiteral("auto") : text(cfg.local_shell));
            shell_path_->setText(text(cfg.local_path));
            local_args_->setText(text(cfg.local_args));
        } else if (kind == 1) {
            network_port_->setValue(int(cfg.ssh_port));
            ssh_timeout_->setValue(int(cfg.ssh_timeout_secs));
            ssh_reconnect_->setChecked(cfg.ssh_reconnect);
            ssh_attempts_->setValue(int(cfg.ssh_reconnect_attempts));
            ssh_interval_->setValue(int(cfg.ssh_reconnect_interval));
            ssh_auth_->setCurrentIndex(cfg.ssh_auth);
            ssh_key_->setText(text(cfg.ssh_key_path));
        } else if (kind == 2) {
            network_port_->setValue(int(cfg.telnet_port));
            loaded_tel_term_ = text(cfg.telnet_term);
        } else if (kind == 3) {
            ser_baud_->setCurrentText(QString::number(cfg.serial_baud));
            ser_bits_->setCurrentText(QString::number(cfg.serial_bits));
            ser_stop_->setCurrentText(QString::number(cfg.serial_stop));
            ser_parity_->setCurrentIndex(cfg.serial_parity);
            ser_flow_->setCurrentIndex(cfg.serial_flow);
        } else if (kind == 4) {
            network_port_->setValue(int(cfg.tcp_port));
            tcp_data_->setCurrentIndex(cfg.tcp_binary ? 1 : 0);
            loaded_tcp_term_ = text(cfg.tcp_term);
        }
    }
    term_->setCurrentText(choice.terminal);
    charset_->setCurrentText(choice.charset);
    load_advanced(choice.options);
    system_->setCurrentText(choice.system);
    sftp_->setChecked(choice.sftp);
    sysmon_->setChecked(choice.sysmon);
    choice_kind_ = kind;
}

void SessionForm::rebuild_nav(int kind) {
    nav_->blockSignals(true);
    nav_->clear();
    auto add_item = [this](const QString &text, int page, bool enabled,
                           QTreeWidgetItem *parent) -> QTreeWidgetItem * {
        auto *item = new QTreeWidgetItem(QStringList{text});
        item->setData(0, Qt::UserRole, page);
        if (parent == nullptr) {
            nav_->addTopLevelItem(item);
        } else {
            parent->addChild(item);
        }
        if (!enabled) item->setDisabled(true);
        return item;
    };
    add_item(i18n::tr("会话"), 0, true, nullptr);
    // Shared terminal/window pages apply to every protocol.
    auto *terminal = add_item(i18n::tr("终端"), -1, true, nullptr);
    for (const QString &leaf : {i18n::tr("响铃"), i18n::tr("模式"), i18n::tr("键盘"),
                                i18n::tr("安全"), i18n::tr("日志")}) {
        add_item(leaf, advanced_pages_.value(leaf), true, terminal);
    }
    auto *window = add_item(i18n::tr("窗口"), -1, true, nullptr);
    add_item(i18n::tr("选择"), advanced_pages_.value(i18n::tr("选择")), true, window);
    if (kind == 0) {
        auto *shell = add_item(QStringLiteral("Shell"), -1, true, nullptr);
        add_item(i18n::tr("进程"), 1, true, shell);
        shell->setExpanded(true);
    } else if (kind == 3) {
        auto *serial = add_item(i18n::tr("串口"), -1, true, nullptr);
        add_item(i18n::tr("协议"), 3, true, serial);
        add_item(QStringLiteral("X/Y/Z Modem"), advanced_pages_.value(QStringLiteral("X/Y/Z Modem")), true, serial);
        serial->setExpanded(true);
    } else if (kind == 1) {
        auto *ssh = add_item(QStringLiteral("SSH"), -1, true, nullptr);
        add_item(i18n::tr("连接设置"), 2, true, ssh);
        // 与 WindTerm SSH 子树同序；无后端项置灰。
        add_item(i18n::tr("代理"), advanced_pages_.value(i18n::tr("代理")), true, ssh);
        add_item(i18n::tr("验证"), 4, true, ssh);
        for (const QString &name : {i18n::tr("代理人"), i18n::tr("加密算法"), i18n::tr("密钥交换"),
                                    i18n::tr("MAC 哈希"), i18n::tr("主机密钥"),
                                    QStringLiteral("SFTP"), QStringLiteral("X11"), QStringLiteral("X/Y/Z Modem")}) {
            const bool enabled = advanced_pages_.contains(name);
            add_item(name, enabled ? advanced_pages_.value(name) : -1, enabled, ssh);
        }
        ssh->setExpanded(true);
    } else if (kind == 4) {
        add_item(QStringLiteral("X/Y/Z Modem"), advanced_pages_.value(QStringLiteral("X/Y/Z Modem")), true, nullptr);
    }
    terminal->setExpanded(true);
    window->setExpanded(true);
    nav_->blockSignals(false);
    nav_->setCurrentItem(nav_->topLevelItem(0));
}
