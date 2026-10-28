// SessionForm: protocol tabs, settings navigation and shared session metadata.
// No moc; the form keeps only widget pointers,
// 保存时一次性收集回 SessCfg（C++ 不持业务状态——widgets 只是表单视图）。
// 布局对齐 WindTerm 新建会话：协议页签 + 左侧设置树 + 「会话」页字段序
// （主字段 → OneKey → 标签 → 分组 → 终端 → 字符集 → 描述），无后端的
// 设置项置灰展示（菜单要接真实操作或置灰）。
#pragma once

#include "cpp/bridge_shim.h" // SessCfg（session 域桥 POD）

#include <QDialog>
#include <QHash>
#include <QVector>
#include <functional>

#include <cstdint>

class QCheckBox;
class QFormLayout;
class QToolButton;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QTabBar;
class QTreeWidget;
class QStackedWidget;

/// 运行会话表单（新建/编辑）。
/// - 新建：`cfg` 传入空白（sess_form_get("", kind) 的产物）；
/// - 编辑：`cfg` 传入回填值（id 非空 → 保存为更新）。
/// 返回 true = 已保存（`cfg` 写回表单值，含 save 后的稳定 id）；
/// `connect_now` 置位表示用户点了「连接」（保存并打开）。
bool run_session_form(QWidget *parent, SessCfg *cfg, bool *connect_now);

/// 自检辅助：构建表单并导出当前输入（不落盘）。offscreen 断言用。
class SessionForm : public QDialog {
public:
    SessionForm(QWidget *parent, const SessCfg &cfg, bool defaults_mode = false);

    void collect(SessCfg *cfg) const;
    /// 自检：协议页数 / 当前协议页名。
    int protocol_pages() const;
    QString current_protocol_label() const;
    /// 自检：SSH 页主机编辑框文本。
    QString ssh_host_text() const;
    /// 「连接」按钮语义（保存并连接；「保存」为 false）。
    bool connect_now_checked() const;

private:
    void build_general();
    QFormLayout *build_settings();
    void build_advanced();
    void load_advanced(const SessOptions &options);
    SessOptions collect_advanced() const;
    struct OptionBinding {
        std::function<void(const SessOptions &)> load;
        std::function<void(SessOptions &)> collect;
    };
    QVector<OptionBinding> option_bindings_;
    QHash<QString, QWidget *> advanced_;
    QHash<QString, int> advanced_pages_;
    void load(const SessCfg &cfg);
    void rebuild_nav(int kind);
    void update_connect_enabled();
    void fit_protocol_fields();
    void switch_network_fields(int kind);
    void select_protocol_defaults(int kind);
    void collect_term(SessCfg *cfg, uint8_t kind) const;

    QFormLayout *common_form_ = nullptr;
    QTabBar *proto_tabs_ = nullptr;
    QTreeWidget *nav_ = nullptr;
    QStackedWidget *settings_ = nullptr;
    // 会话页：协议主字段栈 + 公共行
    QStackedWidget *protocol_fields_ = nullptr;
    QComboBox *shell_ = nullptr;
    QWidget *network_fields_ = nullptr;
    QLineEdit *network_host_ = nullptr;
    QSpinBox *network_port_ = nullptr;
    struct NetworkDraft { QString host; int port = 23; };
    NetworkDraft destinations_[5];
    int network_kind_ = -1;
    QComboBox *onekey_ = nullptr;
    QComboBox *tcp_data_ = nullptr;
    QComboBox *ser_path_ = nullptr;
    QComboBox *ser_baud_ = nullptr;
    QComboBox *ser_bits_ = nullptr;
    QComboBox *ser_stop_ = nullptr;
    QComboBox *ser_parity_ = nullptr;
    QComboBox *ser_flow_ = nullptr;
    // 会话页公共行
    QLineEdit *label_ = nullptr;
    QComboBox *group_ = nullptr;
    QComboBox *term_ = nullptr;
    QComboBox *system_ = nullptr;
    QToolButton *label_color_btn_ = nullptr;
    QCheckBox *sftp_ = nullptr;
    QCheckBox *sysmon_ = nullptr;
    QComboBox *charset_ = nullptr;
    QPlainTextEdit *desc_ = nullptr;
    QCheckBox *favorite_ = nullptr;
    QString label_color_;
    // Shell > 进程页
    QLineEdit *shell_path_ = nullptr;
    QLineEdit *local_args_ = nullptr;
    QLineEdit *local_cwd_ = nullptr;
    // SSH > 验证页
    QComboBox *ssh_auth_ = nullptr;
    QLineEdit *ssh_user_ = nullptr;
    QLineEdit *ssh_password_ = nullptr;
    QLineEdit *ssh_key_ = nullptr;
    QLineEdit *ssh_passphrase_ = nullptr;
    QCheckBox *ssh_remember_ = nullptr;
    // SSH > 连接页
    QSpinBox *ssh_timeout_ = nullptr;
    QSpinBox *ssh_attempts_ = nullptr;
    QSpinBox *ssh_interval_ = nullptr;
    QCheckBox *ssh_reconnect_ = nullptr;
    // 加载值（collect 回写，防编辑重置）
    QString loaded_tel_term_;
    QString loaded_tcp_term_;
    bool connect_now_ = false;
    struct ProtocolChoices {
        QString terminal, system;
        QString charset;
        SessOptions options;
        bool sftp = true, sysmon = false, visited = false;
    };
    ProtocolChoices choices_[5]; // Unsaved widget choices while switching new-form tabs.
    int choice_kind_ = 0;
    bool use_new_defaults_ = false;
};
