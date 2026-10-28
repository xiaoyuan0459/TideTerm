// Shared option controls: one field, one typed binding, all five protocols.
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_form_widgets.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QSpinBox>
#include <QStackedWidget>
#include <QVBoxLayout>
#include <type_traits>

namespace {
QString q(const rust::String &value) { return QString::fromUtf8(value.data(), int(value.size())); }
}

void SessionForm::build_advanced() {
    QHash<QString, QFormLayout *> forms;
    forms.insert(QStringLiteral("连接"), settings_->widget(2)->findChild<QFormLayout *>());
    for (const char *name : {"响铃", "模式", "键盘", "安全", "日志", "选择",
                              "代理", "代理人", "加密算法", "密钥交换", "MAC 哈希", "主机密钥", "SFTP", "X11", "X/Y/Z Modem"}) {
        auto *page = new QWidget(settings_);
        auto *root = new QVBoxLayout(page);
        root->setContentsMargins(0, 0, 0, 0);
        auto *form = new QFormLayout;
        form->setFormAlignment(Qt::AlignTop);
        root->addLayout(form);
        root->addStretch(1);
        forms.insert(QString::fromUtf8(name), form);
        advanced_pages_.insert(i18n::tr(name), settings_->addWidget(page));
    }
    // The same typed member drives loading and collecting; adding an option cannot
    // accidentally wire its read and write paths to different backend fields.
    auto bind = [this](const char *key, QWidget *field, auto member) {
        field->setObjectName(QStringLiteral("sessionOption_") + QString::fromLatin1(key));
        advanced_.insert(QString::fromLatin1(key), field);
        using Value = std::decay_t<decltype(SessOptions{}.*member)>;
        OptionBinding binding;
        if constexpr (std::is_same_v<Value, rust::String>) {
            auto *line = static_cast<QLineEdit *>(field);
            binding.load = [line, member](const SessOptions &options) { line->setText(q(options.*member)); };
            binding.collect = [line, member](SessOptions &options) {
                options.*member = rust::String(line->text().toUtf8().toStdString());
            };
        } else if (auto *check = qobject_cast<QCheckBox *>(field)) {
            binding.load = [check, member](const SessOptions &options) { check->setChecked(bool(options.*member)); };
            binding.collect = [check, member](SessOptions &options) { options.*member = Value(check->isChecked()); };
        } else if (auto *spin = qobject_cast<QSpinBox *>(field)) {
            binding.load = [spin, member](const SessOptions &options) { spin->setValue(int(options.*member)); };
            binding.collect = [spin, member](SessOptions &options) { options.*member = Value(spin->value()); };
        } else {
            auto *combo = static_cast<QComboBox *>(field);
            binding.load = [combo, member](const SessOptions &options) { combo->setCurrentIndex(int(options.*member)); };
            binding.collect = [combo, member](SessOptions &options) { options.*member = Value(combo->currentIndex()); };
        }
        option_bindings_.append(std::move(binding));
    };
    struct CheckSpec { const char *key; const char *page; QString label; bool SessOptions::*member; };
    const CheckSpec checks[] = {
        {"auto_wrap", "模式", i18n::tr("自动换行 (DECAWM)"), &SessOptions::auto_wrap},
        {"newline_mode", "模式", i18n::tr("换行时回到行首 (LNM)"), &SessOptions::newline_mode},
        {"app_cursor", "模式", i18n::tr("应用光标键 (DECCKM)"), &SessOptions::app_cursor},
        {"app_keypad", "模式", i18n::tr("应用数字键盘 (DECNKM)"), &SessOptions::app_keypad},
        {"bell_visual", "响铃", i18n::tr("显示视觉响铃"), &SessOptions::bell_visual},
        {"bell_sound", "响铃", i18n::tr("使用系统提示音"), &SessOptions::bell_sound},
        {"remote_clipboard", "安全", i18n::tr("允许远端读写剪贴板 (OSC 52)"), &SessOptions::remote_clipboard},
        {"remote_title", "安全", i18n::tr("允许远端修改会话标题"), &SessOptions::remote_title},
        {"log_enabled", "日志", i18n::tr("连接时开始捕获会话输出"), &SessOptions::log_enabled},
        {"copy_on_select", "选择", i18n::tr("选择后自动复制"), &SessOptions::copy_on_select},
        {"path_word_selection", "选择", i18n::tr("双击按路径分隔符选择"), &SessOptions::path_word_selection},
        {"mark_save_on_close", "选择", i18n::tr("关闭时保存标记为默认"), &SessOptions::mark_save_on_close},
        {"right_click_paste", "选择", i18n::tr("右键粘贴（Ctrl+右键打开菜单）"), &SessOptions::right_click_paste},
        {"compression", "连接", i18n::tr("启用 SSH 压缩"), &SessOptions::compression},
        {"agent_forwarding", "代理人", i18n::tr("转发本机 SSH Agent"), &SessOptions::agent_forwarding},
        {"x11_enabled", "X11", i18n::tr("启用 X11 转发"), &SessOptions::x11_enabled},
        {"x11_no_auth", "X11", i18n::tr("本机 X Server 无需认证"), &SessOptions::x11_no_auth},
        {"sftp_show_hidden", "SFTP", i18n::tr("显示隐藏文件"), &SessOptions::sftp_show_hidden},
        {"sftp_follow_cwd", "SFTP", i18n::tr("跟随终端工作目录"), &SessOptions::sftp_follow_cwd},
        {"sftp_repair_names", "SFTP", i18n::tr("自动修复无效路径字符"), &SessOptions::sftp_repair_names},
        {"sftp_preserve_modified", "SFTP", i18n::tr("保留原始文件修改时间"), &SessOptions::sftp_preserve_modified},
        {"sftp_preserve_accessed", "SFTP", i18n::tr("保留原始文件访问时间"), &SessOptions::sftp_preserve_accessed},
    };
    for (const auto &spec : checks) {
        auto *field = new QCheckBox(spec.label, this);
        forms.value(QString::fromUtf8(spec.page))->addRow(QString(), field);
        bind(spec.key, field, spec.member);
    }
    struct SpinSpec {
        const char *key; const char *page; QString label;
        uint32_t SessOptions::*member; int minimum; int maximum; int base;
    };
    const SpinSpec spins[] = {
        {"bell_interval_secs", "响铃", i18n::tr("响铃最小间隔（秒）"), &SessOptions::bell_interval_secs, 0, 60, 10},
        {"scrollback_mb", "模式", i18n::tr("回滚内存 (MiB)"), &SessOptions::scrollback_mb, 0, 512, 10},
        {"keepalive_secs", "连接", i18n::tr("保活间隔（秒）"), &SessOptions::keepalive_secs, 0, 3600, 10},
        {"watchdog_secs", "连接", i18n::tr("响应超时（秒）"), &SessOptions::watchdog_secs, 0, 600, 10},
        {"modem_timeout_secs", "X/Y/Z Modem", i18n::tr("响应超时（秒）"), &SessOptions::modem_timeout_secs, 1, 120, 10},
        {"proxy_port", "代理", i18n::tr("端口"), &SessOptions::proxy_port, 1, 65535, 10},
        {"sftp_file_mode", "SFTP", i18n::tr("文件权限（八进制）"), &SessOptions::sftp_file_mode, 0, 0777, 8},
        {"sftp_dir_mode", "SFTP", i18n::tr("目录权限（八进制）"), &SessOptions::sftp_dir_mode, 0, 0777, 8},
    };
    for (const auto &spec : spins) {
        auto *field = new chrome::SessionSpin(this);
        field->setRange(spec.minimum, spec.maximum);
        field->setDisplayIntegerBase(spec.base);
        forms.value(QString::fromUtf8(spec.page))->addRow(spec.label, field);
        bind(spec.key, field, spec.member);
    }
    auto combo = [this, &forms, &bind](const char *key, const char *page, const QString &label,
                                    const QStringList &choices, auto member, int row = -1) {
        auto *field = new chrome::SessionCombo(this);
        field->addItems(choices);
        forms.value(QString::fromUtf8(page))->insertRow(row, label, field);
        bind(key, field, member);
    };
    combo("cursor_style", "模式", i18n::tr("光标样式"), {i18n::tr("块"), i18n::tr("下划线"), i18n::tr("竖线")}, &SessOptions::cursor_style);
    combo("backspace_del", "键盘", i18n::tr("Backspace 发送"), {i18n::tr("BS (Ctrl+H)"), i18n::tr("DEL (Ctrl+?)")}, &SessOptions::backspace_del);
    combo("delete_mode", "键盘", i18n::tr("Delete 发送"), {QStringLiteral("CSI 3~"), i18n::tr("DEL (Ctrl+?)"), i18n::tr("BS (Ctrl+H)")}, &SessOptions::delete_mode);
    combo("enter_mode", "键盘", i18n::tr("Enter 发送"), {QStringLiteral("CR"), QStringLiteral("LF"), QStringLiteral("CR+LF")}, &SessOptions::enter_mode);
    combo("proxy_kind", "代理", i18n::tr("代理类型"), {i18n::tr("无"), QStringLiteral("SOCKS5"), QStringLiteral("HTTP CONNECT")}, &SessOptions::proxy_kind, 0);

    struct TextSpec {
        const char *key; const char *page; QString label;
        rust::String SessOptions::*member; int algorithm = -1; bool secret = false; int row = -1;
    };
    const TextSpec texts[] = {
        {"proxy_host", "代理", i18n::tr("主机"), &SessOptions::proxy_host, -1, false, 1},
        {"proxy_user", "代理", i18n::tr("用户名"), &SessOptions::proxy_user},
        {"proxy_password", "代理", i18n::tr("密码（仅此次连接）"), &SessOptions::proxy_password, -1, true},
        {"sftp_initial_dir", "SFTP", i18n::tr("初始目录"), &SessOptions::sftp_initial_dir, -1, false, 0},
        {"ciphers", "加密算法", i18n::tr("优先顺序（逗号分隔）"), &SessOptions::ciphers, 0},
        {"kex", "密钥交换", i18n::tr("优先顺序（逗号分隔）"), &SessOptions::kex, 1},
        {"macs", "MAC 哈希", i18n::tr("优先顺序（逗号分隔）"), &SessOptions::macs, 2},
        {"host_keys", "主机密钥", i18n::tr("优先顺序（逗号分隔）"), &SessOptions::host_keys, 3},
        {"x11_display", "X11", i18n::tr("本机显示地址"), &SessOptions::x11_display},
        {"x11_auth_file", "X11", i18n::tr("Xauthority 文件"), &SessOptions::x11_auth_file},
    };
    for (const auto &spec : texts) {
        auto *field = new QLineEdit(this);
        auto *form = forms.value(QString::fromUtf8(spec.page));
        form->insertRow(spec.row, spec.label, field);
        if (spec.secret) field->setEchoMode(QLineEdit::Password);
        if (spec.algorithm >= 0) {
            QStringList names;
            for (const auto &name : sess_ssh_algorithms(uint8_t(spec.algorithm))) names.append(q(name));
            field->setPlaceholderText(i18n::tr("留空使用库默认顺序"));
            field->setToolTip(names.join(QStringLiteral(", ")));
            auto *supported = new QLabel(names.join(QStringLiteral("\n")), this);
            supported->setWordWrap(true);
            supported->setTextInteractionFlags(Qt::TextSelectableByMouse);
            form->addRow(i18n::tr("当前支持"), supported);
        }
        bind(spec.key, field, spec.member);
    }
    auto *agent = new QCheckBox(i18n::tr("使用系统 SSH Agent 认证"), this);
    forms.value(QStringLiteral("代理人"))->addRow(QString(), agent);
    connect(agent, &QCheckBox::toggled, this, [this](bool enabled) {
        if (enabled) ssh_auth_->setCurrentIndex(4);
        else if (ssh_auth_->currentIndex() == 4) ssh_auth_->setCurrentIndex(0);
    });
    connect(ssh_auth_, qOverload<int>(&QComboBox::currentIndexChanged), agent,
            [agent](int mode) { agent->setChecked(mode == 4); });
    auto *proxy = static_cast<QComboBox *>(advanced_.value(QStringLiteral("proxy_kind")));
    auto update_proxy = [this, forms](int kind) {
        auto *form = forms.value(QStringLiteral("代理"));
        for (const char *name : {"proxy_host", "proxy_port", "proxy_user", "proxy_password"})
            form->setRowVisible(advanced_.value(QString::fromLatin1(name)), kind != 0);
    };
    connect(proxy, qOverload<int>(&QComboBox::currentIndexChanged), this, update_proxy);
    update_proxy(0);
    static_cast<QLineEdit *>(advanced_.value(QStringLiteral("x11_display")))->setPlaceholderText(QStringLiteral("DISPLAY / 127.0.0.1:0"));
    static_cast<QLineEdit *>(advanced_.value(QStringLiteral("x11_auth_file")))->setPlaceholderText(QStringLiteral("XAUTHORITY / ~/.Xauthority"));
    auto *modem_help = new QLabel(i18n::tr("先在对端启动接收或发送程序，再从工具菜单选择 X/Y/Z Modem。传输期间暂停终端输入；接收不覆盖已有文件。XMODEM 无文件大小信息，接收保留末块填充。"), this);
    modem_help->setWordWrap(true);
    forms.value(QStringLiteral("X/Y/Z Modem"))->addRow(modem_help);
    for (auto *form : forms) chrome::prepare_session_fields(form);
}

void SessionForm::load_advanced(const SessOptions &options) {
    for (const auto &binding : option_bindings_) binding.load(options);
}

SessOptions SessionForm::collect_advanced() const {
    SessOptions options;
    for (const auto &binding : option_bindings_) binding.collect(options);
    return options;
}
