// Protocol tabs and settings navigation; saving still collects the same SessCfg.
// → sess_save（Rust 侧落 sessions.json；凭据 skip_serializing 不落盘）。
// 「记住密码」经 vault 域桥存主密码加密的 vault.json（chrome/session_vault）。
// 布局对齐 WindTerm 新建会话：左侧设置树（无后端项置灰）+ 会话页字段序
// 主字段 → OneKey → 标签 → 分组 → 终端 → 字符集 → 描述；底部
// 编辑默认设置 / 保存 / 连接 / 取消。

#include "cpp/chrome/session_form.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/onekey_dialog.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/session_form_widgets.h"
#include "cpp/chrome/app_font.h"
#include "cpp/i18n.h"

#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QMessageBox>
#include <QSet>
#include <QSpinBox>
#include <QStandardItemModel>
#include <QTabBar>
#include <QStackedWidget>
#include <QToolButton>
#include <QTreeWidget>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }

QString localized_validation_error(const rust::String &message) {
    const QString error = q(message);
    struct Message { const char *diagnostic; QString text; };
    const Message messages[] = {
        {"invalid terminal options", i18n::tr("终端选项无效")},
        {"Modem timeout must be 1–120 seconds", i18n::tr("Modem 超时必须为 1–120 秒")},
        {"invalid X11 display or authority path", i18n::tr("X11 显示地址或认证文件路径无效")},
        {"invalid SFTP creation permissions", i18n::tr("SFTP 新建文件或目录的权限无效")},
        {"invalid SSH keepalive/watchdog interval", i18n::tr("SSH 保活或响应超时间隔无效")},
        {"proxy requires a host and port", i18n::tr("代理需要主机地址和端口")},
        {"invalid proxy address or credentials", i18n::tr("代理地址或凭据无效")},
        {"SSH algorithms must be a comma-separated name list", i18n::tr("SSH 算法须为逗号分隔的名称列表")},
        {"unsupported SSH algorithm preference", i18n::tr("不支持所选 SSH 算法")},
        {"invalid SFTP initial directory", i18n::tr("SFTP 初始目录无效")},
        {"invalid proxy port", i18n::tr("代理端口无效")},
        {"invalid reconnect policy", i18n::tr("自动重连设置无效")},
        {"unsupported character set", i18n::tr("不支持所选字符集")},
        {"terminal character set must preserve ASCII controls", i18n::tr("终端字符集必须保留 ASCII 控制字符")},
    };
    for (const auto &entry : messages) {
        if (error == QString::fromUtf8(entry.diagnostic)) return entry.text;
    }
    return error;
}
}

SessionForm::SessionForm(QWidget *parent, const SessCfg &cfg, bool defaults_mode) : QDialog(parent) {
    setObjectName(QStringLiteral("sessionForm"));
    setWindowTitle(cfg.id.empty() ? i18n::tr("新建会话")
                                  : i18n::tr("编辑会话"));
    auto *root = new QVBoxLayout(this);
    root->setContentsMargins(0, 0, 0, 10);
    root->setSpacing(0);
    resize(640, 612);
    setMinimumSize(600, 570);
    proto_tabs_ = new QTabBar(this);
    proto_tabs_->setObjectName(QStringLiteral("sessionProtocols"));
    proto_tabs_->setExpanding(true);
    proto_tabs_->setDrawBase(false);
    proto_tabs_->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    root->addWidget(proto_tabs_);

    auto *content = new QHBoxLayout;
    content->setContentsMargins(10, 10, 10, 12);
    content->setSpacing(12);
    nav_ = new QTreeWidget(this);
    nav_->setObjectName(QStringLiteral("sessionSettingsNav"));
    nav_->header()->hide();
    nav_->setRootIsDecorated(true);
    nav_->setFixedWidth(208);
    nav_->setIndentation(18);
    nav_->setUniformRowHeights(true);
    content->addWidget(nav_);
    settings_ = new QStackedWidget(this);
    content->addWidget(settings_, 1);
    root->addLayout(content, 1);

    build_general();
    QFormLayout *auth_form = build_settings();
    build_advanced();

    // ─────────── 协议页签（展示序 = 页序；tabData 存 kind） ───────────
    // 页序 Shell SSH Telnet Tcp Serial ↔ kind 0 1 2 4 3（cxx：3=serial 4=tcp）。
    const int page_kind[] = {0, 1, 2, 4, 3};
    const QStringList labels = {QStringLiteral("Shell"), QStringLiteral("SSH"),
        QStringLiteral("Telnet"), QStringLiteral("TCP"), i18n::tr("串口")};
    for (int page = 0; page < 5; ++page) {
        const int index = proto_tabs_->addTab(labels[page]);
        proto_tabs_->setTabData(index, page_kind[page]);
    }

    // ─────────── 导航树（随协议页签重建；无后端项置灰） ───────────
    auto show_page = [this](int page) {
        if (page >= 0) settings_->setCurrentIndex(page);
    };
    connect(nav_, &QTreeWidget::currentItemChanged, this,
            [show_page](QTreeWidgetItem *cur, QTreeWidgetItem *) {
                if (cur != nullptr) show_page(cur->data(0, Qt::UserRole).toInt());
            });

    // kind → 主字段栈下标（栈页序 = 展示序，kind 3=serial 在页 4、4=tcp 在页 3）。
    auto page_of_kind = [](int kind) {
        static const int kPageByKind[] = {0, 1, 2, 4, 3};
        return kPageByKind[kind % 5];
    };
    // 协议页签切换：主字段栈 + 导航树。
    auto update_protocol = [this, show_page, page_of_kind](int index) {
        const int kind = proto_tabs_->tabData(index).toInt();
        switch_network_fields(kind);
        if (use_new_defaults_) select_protocol_defaults(kind);
        protocol_fields_->setCurrentIndex(page_of_kind(kind));
        fit_protocol_fields();
        sftp_->setVisible(kind == 1);
        common_form_->setRowVisible(sftp_, kind == 1);
        sysmon_->setEnabled(kind == 0 || kind == 1);
        sysmon_->setToolTip(kind == 0 || kind == 1 ? i18n::tr("显示系统 CPU 和内存用量") : i18n::tr("该协议不支持系统监控"));
        rebuild_nav(kind);
        show_page(0);
        update_connect_enabled();
    };
    connect(proto_tabs_, &QTabBar::currentChanged, this, update_protocol);

    // 验证页行可见性：0=密码 1=密钥 2=键盘交互 3=OneKey。
    auto update_auth = [this, auth_form, defaults_mode](int kind) {
        auto visible = [auth_form](QWidget *w, bool on) {
            if (QWidget *label = auth_form->labelForField(w)) label->setVisible(on);
            w->setVisible(on);
        };
        visible(ssh_user_, !defaults_mode && kind != 3);
        visible(ssh_password_, !defaults_mode && kind == 0);
        visible(ssh_key_, kind == 1);
        visible(ssh_passphrase_, !defaults_mode && kind == 1);
        visible(ssh_remember_, !defaults_mode && (kind == 0 || kind == 1));
    };
    connect(ssh_auth_, qOverload<int>(&QComboBox::currentIndexChanged), this, update_auth);
    // OneKey 行与验证页「认证方式」联动：选账户 → OneKey 认证。
    connect(onekey_, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this, update_auth](int index) {
                if (index > 0 && ssh_auth_->currentIndex() != 3) {
                    ssh_auth_->setCurrentIndex(3);
                    update_auth(3);
                }
            });
    connect(ssh_auth_, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this](int kind) {
                if (kind != 3 && onekey_->currentIndex() > 0) {
                    onekey_->blockSignals(true);
                    onekey_->setCurrentIndex(0);
                    onekey_->blockSignals(false);
                }
            });

    auto validate_accept = [this] {
        SessCfg draft; collect(&draft);
        const auto error = localized_validation_error(sess_validate(draft));
        if (error.isEmpty()) accept();
        else QMessageBox::warning(this, i18n::tr("配置无效"), error);
    };
    // ─────────── 底部：默认设置 / 保存 / 连接 / 取消 ───────────
    auto *footer = new QHBoxLayout;
    auto *defaults = new QPushButton(i18n::tr("编辑默认设置...(E)"), this);
    defaults->setToolTip(i18n::tr("默认设置用于之后新建的会话"));
    connect(defaults, &QPushButton::clicked, this, [this] {
        const auto kind = uint8_t(proto_tabs_->tabData(proto_tabs_->currentIndex()).toUInt());
        SessCfg draft = sess_defaults_get(kind);
        SessionForm editor(this, draft, true);
        if (editor.exec() != QDialog::Accepted) return;
        editor.collect(&draft);
        const QString error = localized_validation_error(sess_defaults_save(draft));
        if (!error.isEmpty()) QMessageBox::warning(this, i18n::tr("保存失败"), error);
    });
    footer->setContentsMargins(10, 0, 10, 0);
    footer->setSpacing(8);
    footer->addWidget(defaults);
    footer->addStretch(1);
    auto *save = new QPushButton(i18n::tr("保存"), this);
    connect(save, &QPushButton::clicked, this, [this, validate_accept] {
        connect_now_ = false;
        validate_accept();
    });
    footer->addWidget(save);
    auto *connect_btn = new QPushButton(i18n::tr("连接"), this);
    connect_btn->setObjectName(QStringLiteral("sessionConnect"));
    connect_btn->setDefault(true);
    connect(connect_btn, &QPushButton::clicked, this, [this, validate_accept] {
        connect_now_ = true;
        validate_accept();
    });
    footer->addWidget(connect_btn);
    auto *cancel = new QPushButton(i18n::tr("取消"), this);
    connect(cancel, &QPushButton::clicked, this, &QDialog::reject);
    footer->addWidget(cancel);
    for (auto *button : {defaults, save, connect_btn, cancel}) button->setFixedHeight(24);
    connect_btn->setMinimumWidth(78);
    cancel->setMinimumWidth(78);
    root->addLayout(footer);

    // 主字段变化 → 连接按钮可用性。
    connect(network_host_, &QLineEdit::textChanged, this,
            [this] { update_connect_enabled(); });
    connect(shell_, &QComboBox::editTextChanged, this,
            [this] { update_connect_enabled(); });
    connect(ser_path_, &QComboBox::currentTextChanged, this,
            [this] { update_connect_enabled(); });

    // 按配置 kind 选初始协议页（tabData == kind）。
    const uint8_t k = cfg.kind < 5 ? cfg.kind : 0;
    for (int i = 0; i < proto_tabs_->count(); ++i) {
        if (proto_tabs_->tabData(i).toInt() == k) {
            proto_tabs_->setCurrentIndex(i);
            break;
        }
    }

    for (auto *field : findChildren<QComboBox *>()) chrome::set_connection_font(field);
    chrome::set_connection_font(proto_tabs_);
    load(cfg);
    choice_kind_ = k;
    use_new_defaults_ = cfg.id.empty() && !defaults_mode;
    choices_[k].visited = true;
    update_protocol(proto_tabs_->currentIndex());
    update_auth(ssh_auth_->currentIndex());
    if (defaults_mode) {
        setWindowTitle(i18n::tr("编辑默认设置"));
        defaults->hide();
        connect_btn->hide();
        save->setDefault(true);
        if (auto *model = qobject_cast<QStandardItemModel *>(ssh_auth_->model()))
            model->item(3)->setEnabled(false); // Preserve the stable Agent auth code (4).
        for (int i = 0; i < proto_tabs_->count(); ++i) proto_tabs_->setTabEnabled(i, i == proto_tabs_->currentIndex());
        for (auto *field : {label_->parentWidget(), static_cast<QWidget *>(group_),
                           static_cast<QWidget *>(desc_), static_cast<QWidget *>(favorite_)}) common_form_->setRowVisible(field, false);
        // Defaults carry options, never destinations, accounts or credentials.
        for (auto *field : {network_host_, ssh_user_, ssh_password_, ssh_passphrase_}) {
            if (auto *form = dynamic_cast<QFormLayout *>(field->parentWidget()->layout())) form->setRowVisible(field, false);
        }
        for (auto *field : {static_cast<QWidget *>(onekey_->parentWidget()), static_cast<QWidget *>(ser_path_)}) {
            if (auto *form = dynamic_cast<QFormLayout *>(field->parentWidget()->layout())) form->setRowVisible(field, false);
        }
        local_cwd_->setEnabled(false);
        local_cwd_->setToolTip(i18n::tr("工作目录在具体会话中设置"));
        ssh_remember_->hide();
        fit_protocol_fields();
    }
}

int SessionForm::protocol_pages() const {
    return proto_tabs_->count();
}

QString SessionForm::current_protocol_label() const {
    return proto_tabs_->tabText(proto_tabs_->currentIndex());
}

QString SessionForm::ssh_host_text() const {
    return network_kind_ == 1 ? network_host_->text() : destinations_[1].host;
}

bool SessionForm::connect_now_checked() const {
    return connect_now_;
}

void SessionForm::update_connect_enabled() {
    const int kind = proto_tabs_->tabData(proto_tabs_->currentIndex()).toInt();
    bool ready = true;
    switch (kind) {
        case 1: case 2: case 4: ready = !network_host_->text().trimmed().isEmpty(); break;
        case 3: ready = !ser_path_->currentText().trimmed().isEmpty(); break;
        default: break; // Empty local shell means automatic selection.
    }
    if (auto *btn = findChild<QPushButton *>(QStringLiteral("sessionConnect"))) {
        btn->setEnabled(ready);
    }
}

bool run_session_form(QWidget *parent, SessCfg *cfg, bool *connect_now) {
    SessionForm form(parent, *cfg);
    if (form.exec() != QDialog::Accepted) {
        return false;
    }
    form.collect(cfg);
    // 记住密码且有要存的凭据：先确保凭据库可用（设置/解锁）；用户取消则不记住，
    // 会话照常保存。字段为空 = 表单未改动（表单永不回填密码），无需解锁。
    const bool will_store = cfg->ssh_remember
        && (!cfg->ssh_password.empty() || !cfg->ssh_passphrase.empty());
    if (will_store && !chrome::ensure_vault_ready(parent)) {
        cfg->ssh_remember = false;
    }
    const rust::String id = sess_save(*cfg);
    if (id.empty()) {
        return false;
    }
    cfg->id = id;
    chrome::sync_saved_credentials(*cfg, id);
    if (connect_now != nullptr) {
        *connect_now = form.connect_now_checked();
    }
    return true;
}
