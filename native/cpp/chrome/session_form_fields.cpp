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

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

QWidget *wrap(const QString &title, QWidget *body, QWidget *parent) {
    auto *w = new QWidget(parent);
    auto *lay = new QVBoxLayout(w);
    lay->setContentsMargins(0, 0, 0, 0);
    lay->addWidget(body);
    w->setWindowTitle(title); // 自检读协议页名用
    return w;
}

/// QFormLayout → 页容器（wrap 只收 widget）。
QWidget *form_page(QFormLayout *f, QWidget *parent) {
    auto *w = new QWidget(parent);
    chrome::prepare_session_fields(f);
    w->setLayout(f);
    return w;
}

/// 现有分组路径（sess_tree_rows 的分组行按深度拼 `>` 路径，去重）。
QStringList existing_groups() {
    QStringList paths;
    QSet<QString> seen;
    QStringList stack;
    for (const SessRow &row : sess_tree_rows()) {
        if (row.kind != 0) continue;
        const int depth = qMin(int(row.depth), stack.size());
        stack.resize(depth);
        const QString name = q(row.label);
        if (name.isEmpty()) continue;
        QString path = name;
        for (int i = 0; i < depth; ++i) {
            if (!stack[i].isEmpty()) path = stack[i] + QStringLiteral(">") + path;
        }
        if (!seen.contains(path)) {
            seen.insert(path);
            paths.append(path);
        }
        stack.append(path);
    }
    return paths;
}

} // namespace

void SessionForm::build_general() {
    // ─────────── 页 0：会话（协议主字段栈 + 公共行） ───────────
    auto *general = new QWidget(settings_);
    auto *general_layout = new QVBoxLayout(general);
    general_layout->setContentsMargins(0, 0, 0, 0);
    general_layout->setSpacing(6);
    network_fields_ = new QWidget(general);
    auto *network = new QFormLayout(network_fields_);
    network->setContentsMargins(0, 0, 0, 0);
    network_host_ = new QLineEdit(network_fields_);
    network_host_->setObjectName(QStringLiteral("sessionHost"));
    network_host_->setPlaceholderText(i18n::tr("[用户@]主机地址"));
    network_port_ = new chrome::SessionSpin(network_fields_);
    network_port_->setObjectName(QStringLiteral("sessionPort"));
    network_port_->setRange(1, 65535);
    network->addRow(i18n::tr("主机:(H)"), network_host_);
    network->addRow(i18n::tr("端口:(P)"), network_port_);
    chrome::prepare_session_fields(network);
    general_layout->addWidget(network_fields_);
    protocol_fields_ = new QStackedWidget(general);
    general_layout->addWidget(protocol_fields_);

    // ---- 本地主字段：Shell:(S) ----
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        shell_ = new chrome::SessionCombo(this);
        shell_->setEditable(true);
        shell_->addItems({QStringLiteral("auto"), QStringLiteral("pwsh"),
                          QStringLiteral("powershell"), QStringLiteral("cmd"),
                          QStringLiteral("git-bash")});
        f->addRow(QStringLiteral("Shell:(S)"), shell_);
        protocol_fields_->addWidget(wrap(QStringLiteral("Shell"), form_page(f, this), this));
    }
    // ---- SSH 主字段：主机/端口/OneKey ----
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        onekey_ = new chrome::SessionCombo(this);
        onekey_->setObjectName(QStringLiteral("sessionOneKey"));
        // OneKey 行数据源 = onekey 域桥账户表（「无」= 不用 OneKey）。
        auto refill_onekey = [this] {
            const QString current = onekey_->currentData().toString();
            onekey_->blockSignals(true);
            onekey_->clear();
            onekey_->addItem(i18n::tr("无"), QString());
            for (const OneKeyRow &row : onekey_rows()) {
                onekey_->addItem(q(row.name) + QStringLiteral(" (%1)").arg(q(row.username)),
                                 q(row.id));
            }
            const int back = onekey_->findData(current);
            onekey_->setCurrentIndex(qMax(0, back));
            onekey_->blockSignals(false);
        };
        refill_onekey();
        auto *onekey_row = new QWidget(this);
        auto *onekey_lay = new QHBoxLayout(onekey_row);
        onekey_lay->setContentsMargins(0, 0, 0, 0);
        onekey_lay->setSpacing(0);
        onekey_lay->addWidget(onekey_, 1);
        onekey_->setObjectName(QStringLiteral("sessionOneKey"));
        auto *gear = new chrome::SessionFieldButton(onekey_);
        gear->setToolTip(i18n::tr("管理 OneKey 账户"));
        chrome::set_button_icon(gear, QStringLiteral("settings"), 12);
        gear->setFixedSize(20, 22);
        auto *onekey_label = new QLabel(QStringLiteral("OneKey:"), this);
        onekey_label->setStyleSheet(QStringLiteral("color: darkorange;"));
        f->addRow(onekey_label, onekey_row);
        connect(gear, &QToolButton::clicked, this,
                [this, refill_onekey] { run_onekey_dialog(this); refill_onekey(); });
        protocol_fields_->addWidget(wrap(QStringLiteral("SSH"), form_page(f, this), this));
    }
    // ---- Telnet / Tcp 主字段：主机/端口 ----
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        protocol_fields_->addWidget(wrap(QStringLiteral("Telnet"), form_page(f, this), this));
    }
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        tcp_data_ = new chrome::SessionCombo(this);
        tcp_data_->addItems({i18n::tr("文本"), i18n::tr("二进制")});
        f->addRow(i18n::tr("数据:(D)"), tcp_data_);
        protocol_fields_->addWidget(wrap(QStringLiteral("Tcp"), form_page(f, this), this));
    }
    // ---- Serial 主字段：串口选择（波特率等在导航「串口 > 协议」页） ----
    {
        auto *f = new QFormLayout;
        f->setFormAlignment(Qt::AlignTop);
        ser_path_ = new chrome::SessionCombo(this);
        ser_path_->setEditable(true);
        for (int i = 1; i <= 8; ++i) {
            ser_path_->addItem(QStringLiteral("COM%1").arg(i));
        }
        f->addRow(i18n::tr("串口:(S)"), ser_path_);
        protocol_fields_->addWidget(wrap(QStringLiteral("Serial"), form_page(f, this), this));
    }

    // ---- 会话页公共行（标签/分组/终端/字符集/描述/收藏） ----
    auto *common = new QFormLayout;
    common_form_ = common;
    common->setFormAlignment(Qt::AlignTop);
    common->setSpacing(6);
    label_color_ = QStringLiteral("#8060df"); // load() 按已存配置覆盖
    auto *label_row = new QWidget(this);
    label_row->setObjectName(QStringLiteral("sessionLabelRow"));
    auto *label_lay = new QHBoxLayout(label_row);
    label_lay->setContentsMargins(0, 0, 0, 0);
    label_lay->setSpacing(4);
    label_ = new QLineEdit(label_row);
    label_->setObjectName(QStringLiteral("sessionLabel"));
    label_lay->addWidget(label_, 1);
    auto *label_clear = new QToolButton(label_row);
    label_clear->setToolTip(i18n::tr("清空标签"));
    chrome::set_button_icon(label_clear, QStringLiteral("clear_input"), 12);
    label_clear->setFixedSize(20, 22);
    label_lay->addWidget(label_clear);
    label_color_btn_ = new QToolButton(label_row);
    auto *label_color_btn = label_color_btn_;
    label_color_btn->setToolTip(i18n::tr("标签颜色"));
    label_color_btn->setFixedSize(20, 20);
    label_lay->addWidget(label_color_btn);
    connect(label_clear, &QToolButton::clicked, this, [this] {
        label_->clear();
        label_->setFocus();
    });
    connect(label_color_btn, &QToolButton::clicked, this, [this, label_color_btn] {
        const QColor picked = QColorDialog::getColor(QColor(label_color_), this,
                                                     i18n::tr("标签颜色"));
        if (picked.isValid()) {
            label_color_ = picked.name(QColor::HexRgb);
            label_color_btn->setStyleSheet(QStringLiteral("background: %1; border: 6px solid %2; border-radius: 0;")
                .arg(label_color_, chrome::ui_colors().term_bg.name()));
        }
    });
    label_color_btn->setStyleSheet(
        QStringLiteral("background: %1; border-radius: 3px;").arg(label_color_));
    common->addRow(i18n::tr("标签:(L)"), label_row);
    group_ = new chrome::SessionCombo(this);
    group_->setEditable(true);
    group_->lineEdit()->setPlaceholderText(i18n::tr("[嵌套组] a>b>c"));
    group_->addItem(QString());
    for (const QString &path : existing_groups()) group_->addItem(path);
    common->addRow(i18n::tr("分组:(G)"), group_);
    term_ = new chrome::SessionCombo(this);
    term_->setEditable(true);
    term_->addItems({QStringLiteral("xterm-256color"), QStringLiteral("xterm"),
                     QStringLiteral("vt100"), QStringLiteral("vt102"),
                     QStringLiteral("dumb")});
    common->addRow(i18n::tr("终端:(T)"), term_);
    system_ = new chrome::SessionCombo(this);
    system_->setObjectName(QStringLiteral("sessionSystem"));
    system_->addItems({QStringLiteral("linux"), QStringLiteral("cmd"), QStringLiteral("powershell"), QStringLiteral("dumb")});
    system_->setEnabled(false);
    system_->setToolTip(i18n::tr("系统类型由连接确定；SSH 系统监控当前支持 Linux"));
    common->addRow(i18n::tr("系统:(S)"), system_);
    charset_ = new chrome::SessionCombo(this);
    charset_->addItems({QStringLiteral("UTF-8"), QStringLiteral("GBK"), QStringLiteral("GB18030"),
        QStringLiteral("Big5"), QStringLiteral("Shift_JIS"), QStringLiteral("EUC-KR"), QStringLiteral("windows-1252")});
    common->addRow(i18n::tr("字符集:(C)"), charset_);
    desc_ = new QPlainTextEdit(this);
    desc_->setFixedHeight(96);
    desc_->setObjectName(QStringLiteral("sessionDescription"));

    common->addRow(i18n::tr("描述:(D)"), desc_);
    sftp_ = new chrome::SessionSwitch(this);
    sftp_->setObjectName(QStringLiteral("sessionSftp"));
    common->addRow(QStringLiteral("SFTP:"), sftp_);
    sysmon_ = new chrome::SessionSwitch(this);
    sysmon_->setObjectName(QStringLiteral("sessionSysmon"));
    common->addRow(i18n::tr("系统监控:"), sysmon_);
    sysmon_->setAccessibleName(i18n::tr("系统监控"));
    sftp_->setAccessibleName(QStringLiteral("SFTP"));
    chrome::prepare_session_fields(common);
    if (auto *label = dynamic_cast<QLabel *>(common->labelForField(desc_))) label->setAlignment(Qt::AlignTop | Qt::AlignRight);
    favorite_ = new QCheckBox(i18n::tr("收藏"), this);
    common->addRow(QString(), favorite_);
    general_layout->addLayout(common);
    general_layout->addStretch(1);
    settings_->addWidget(wrap(i18n::tr("会话"), general, settings_));

}
