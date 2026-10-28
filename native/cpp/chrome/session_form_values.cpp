// Shared form values and per-protocol destination drafts.
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/ui_theme.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QSpinBox>
#include <QStackedWidget>
#include <QTabBar>
#include <QToolButton>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
}

void SessionForm::switch_network_fields(int kind) {
    if (network_kind_ >= 0 && network_kind_ < 5)
        destinations_[network_kind_] = {network_host_->text(), network_port_->value()};
    if (kind < 0 || kind >= 5) kind = 0;
    network_kind_ = kind;
    network_host_->setText(destinations_[kind].host);
    network_port_->setValue(destinations_[kind].port);
    network_fields_->setVisible(kind == 1 || kind == 2 || kind == 4);
}

void SessionForm::load(const SessCfg &cfg) {
    load_advanced(cfg.options);
    label_->setText(q(cfg.name));
    label_color_ = q(cfg.label_color).isEmpty() ? QStringLiteral("#8060df")
                                                : q(cfg.label_color);
    label_color_btn_->setStyleSheet(QStringLiteral("background: %1; border: 6px solid %2; border-radius: 0;")
        .arg(label_color_, chrome::ui_colors().term_bg.name()));
    group_->setCurrentText(q(cfg.group_path));
    system_->setCurrentText(q(cfg.system_shell).isEmpty() ? (cfg.kind == 1 ? QStringLiteral("linux") : QStringLiteral("powershell")) : q(cfg.system_shell));
    sftp_->setChecked(cfg.sftp_enabled);
    sysmon_->setChecked(cfg.sysmon_enabled);
    desc_->setPlainText(q(cfg.description));
    favorite_->setChecked(cfg.favorite);
    if (charset_->findText(q(cfg.charset)) < 0 && !cfg.charset.empty()) {
        charset_->addItem(q(cfg.charset));
    }
    charset_->setCurrentText(q(cfg.charset).isEmpty() ? QStringLiteral("UTF-8")
                                                      : q(cfg.charset));

    shell_->setCurrentText(cfg.local_shell.empty() ? QStringLiteral("auto") : q(cfg.local_shell));
    shell_path_->setText(q(cfg.local_path));
    local_args_->setText(q(cfg.local_args));
    local_cwd_->setText(q(cfg.local_cwd));

    destinations_[1] = {q(cfg.ssh_host), int(cfg.ssh_port == 0 ? 22 : cfg.ssh_port)};
    destinations_[2] = {q(cfg.telnet_host), int(cfg.telnet_port == 0 ? 23 : cfg.telnet_port)};
    destinations_[4] = {q(cfg.tcp_host), int(cfg.tcp_port == 0 ? 23 : cfg.tcp_port)};
    network_kind_ = -1;
    switch_network_fields(cfg.kind);
    ssh_user_->setText(q(cfg.ssh_user));
    ssh_auth_->setCurrentIndex(cfg.ssh_auth < 5 ? cfg.ssh_auth : 0);
    ssh_password_->setText(q(cfg.ssh_password));
    ssh_key_->setText(q(cfg.ssh_key_path));
    ssh_passphrase_->setText(q(cfg.ssh_passphrase));
    ssh_timeout_->setValue(int(cfg.ssh_timeout_secs == 0 ? 15 : cfg.ssh_timeout_secs));
    ssh_reconnect_->setChecked(cfg.ssh_reconnect);
    ssh_attempts_->setValue(int(cfg.ssh_reconnect_attempts));
    ssh_interval_->setValue(int(cfg.ssh_reconnect_interval));
    ssh_remember_->setChecked(cfg.ssh_remember);


    ser_path_->setCurrentText(q(cfg.serial_path));
    ser_baud_->setCurrentText(
        cfg.serial_baud == 0 ? QStringLiteral("115200")
                             : QString::number(cfg.serial_baud));
    ser_bits_->setCurrentText(QString::number(cfg.serial_bits == 0 ? 8 : cfg.serial_bits));
    ser_stop_->setCurrentText(QString::number(cfg.serial_stop == 0 ? 1 : cfg.serial_stop));
    ser_parity_->setCurrentIndex(cfg.serial_parity < 3 ? cfg.serial_parity : 0);
    ser_flow_->setCurrentIndex(cfg.serial_flow < 3 ? cfg.serial_flow : 0);

    tcp_data_->setCurrentIndex(cfg.tcp_binary ? 1 : 0);

    // 终端类型随协议回填：SSH/本地/串口 → meta.terminal_emulation。
    const uint8_t k = cfg.kind < 5 ? cfg.kind : 0;
    const QString term =
        k == 2 ? (cfg.telnet_term.empty() ? QStringLiteral("xterm") : q(cfg.telnet_term))
        : k == 4 ? (cfg.tcp_term.empty() ? QStringLiteral("xterm") : q(cfg.tcp_term))
        : (cfg.terminal_emulation.empty() ? QStringLiteral("xterm-256color")
                                          : q(cfg.terminal_emulation));
    if (term_->findText(term) < 0) term_->addItem(term);
    term_->setCurrentText(term);
    loaded_tel_term_ = q(cfg.telnet_term);
    loaded_tcp_term_ = q(cfg.tcp_term);
}

void SessionForm::fit_protocol_fields() {
    if (auto *form = protocol_fields_->currentWidget()->findChild<QFormLayout *>()) {
        form->invalidate();
        protocol_fields_->setFixedHeight(form->totalSizeHint().height());
    }
}

void SessionForm::collect_term(SessCfg *cfg, uint8_t kind) const {
    const QString text = term_->currentText().trimmed();
    if (kind == 2) {
        cfg->telnet_term = rs(text.isEmpty() ? QStringLiteral("xterm") : text);
    } else if (kind == 4) {
        cfg->tcp_term = rs(text.isEmpty() ? QStringLiteral("xterm") : text);
    } else {
        cfg->terminal_emulation =
            rs(text.isEmpty() ? QStringLiteral("xterm-256color") : text);
    }
}

void SessionForm::collect(SessCfg *cfg) const {
    auto collect_destination = [this](int kind, rust::String &host, uint32_t &port) {
        const NetworkDraft draft = network_kind_ == kind
            ? NetworkDraft{network_host_->text(), network_port_->value()} : destinations_[kind];
        host = rust::String(draft.host.trimmed().toUtf8().toStdString());
        port = uint32_t(draft.port);
    };
    cfg->options = collect_advanced();
    const uint8_t kind = uint8_t(proto_tabs_->tabData(proto_tabs_->currentIndex()).toUInt());
    const QString name = label_->text().trimmed();
    // WindTerm 语义：标签即会话显示名（label 跟随 name，保持侧栏徽标一致）。
    cfg->name = rs(name);
    cfg->label = rs(name);
    cfg->label_color = rs(label_color_);
    cfg->group_path = rs(group_->currentText().trimmed());
    cfg->description = rs(desc_->toPlainText());
    cfg->favorite = favorite_->isChecked();
    cfg->charset = rs(charset_->currentText().isEmpty() ? QStringLiteral("UTF-8")
                                                        : charset_->currentText());
    cfg->system_shell = rs(system_->currentText());
    cfg->sftp_enabled = sftp_->isChecked();
    cfg->sysmon_enabled = sysmon_->isChecked();
    cfg->kind = kind;
    collect_term(cfg, kind);

    cfg->local_shell = rs(shell_->currentText());
    cfg->local_path = rs(shell_path_->text().trimmed());
    cfg->local_args = rs(local_args_->text());
    cfg->local_cwd = rs(local_cwd_->text().trimmed());

    collect_destination(1, cfg->ssh_host, cfg->ssh_port);
    cfg->ssh_user = rs(ssh_user_->text().trimmed());
    cfg->ssh_auth = uint8_t(ssh_auth_->currentIndex());
    cfg->ssh_account_id = rs(onekey_->currentData().toString());
    cfg->ssh_password = rs(ssh_password_->text());
    cfg->ssh_key_path = rs(ssh_key_->text().trimmed());
    cfg->ssh_passphrase = rs(ssh_passphrase_->text());
    cfg->ssh_timeout_secs = uint32_t(ssh_timeout_->value());
    cfg->ssh_reconnect = ssh_reconnect_->isChecked();
    cfg->ssh_reconnect_attempts = uint32_t(ssh_attempts_->value());
    cfg->ssh_reconnect_interval = uint32_t(ssh_interval_->value());
    cfg->ssh_remember = ssh_remember_->isChecked();

    collect_destination(2, cfg->telnet_host, cfg->telnet_port);

    cfg->serial_path = rs(ser_path_->currentText());
    cfg->serial_baud = ser_baud_->currentText().toUInt();
    cfg->serial_bits = ser_bits_->currentText().toUInt();
    cfg->serial_stop = ser_stop_->currentText().toUInt();
    cfg->serial_parity = uint8_t(ser_parity_->currentIndex());
    cfg->serial_flow = uint8_t(ser_flow_->currentIndex());

    collect_destination(4, cfg->tcp_host, cfg->tcp_port);
    cfg->tcp_binary = tcp_data_->currentIndex() == 1;
}
