// AiPanel —— AI 助手面板（Phase C ⑥；对齐 Slint 壳 ai_panel.slint）：
// 自然语言 → 命令候选（可发送到终端）/ 错误解释 / provider 健康检查。
// 无 moc（std::function 回调；结果经 ai_take_replies 泵轮询）。
#pragma once

#include <QWidget>

#include <functional>

class MainWindow;

/// AI 助手面板（Phase C ⑥；对齐 Slint 壳 ai_panel.slint）：

class QLabel;
class QLineEdit;
class QListWidget;
class QPushButton;
class QPlainTextEdit;
class QComboBox;

class AiPanel : public QWidget {
public:
    explicit AiPanel(QWidget *parent = nullptr);

    /// 泵轮询（MainWindow 20ms tick 调）：领取生成结果并渲染。
    void poll();

    /// 直接发起（菜单 ai:natural-language / ai:explain-error 的入口）。
    void ask_command(const QString &prompt);
    void ask_explain(const QString &error_text);

    /// 「发送到终端」出口（MainWindow 注入：写活动窗格 PTY）。
    std::function<void(const QString &command)> on_send_command;
    /// 状态反馈出口。
    std::function<void(const QString &msg)> on_note;

    /// 状态标签（菜单入口设置提示文本用）。
    QLabel *status() const { return status_; }

    // ---- 自检 ----
    int candidate_count() const;
    QString explanation_text() const;
    bool busy() const;

private:
    void run(bool explain);
    void render_reply(int kind, bool ok, const QString &explanation,
                      const QStringList &commands, const QStringList &notes,
                      const QVector<int> &confidences);

    QComboBox *mode_ = nullptr;
    QLineEdit *input_ = nullptr;
    QPushButton *ask_ = nullptr;
    QListWidget *candidates_ = nullptr;
    QPlainTextEdit *explanation_ = nullptr;
    QLabel *status_ = nullptr;
    bool busy_ = false;
};

/// AI 设置对话框（provider/base_url/model/隐私模式/API key——key 只进内存）。
/// 返回 true = 已保存。
bool run_ai_settings(QWidget *parent);

/// 装配 AI 面板回调（发送到终端/状态反馈）——MainWindow ctor 一行接线，
/// AI 域装配逻辑归本模块（main_window.cpp 行数守恒）。
void wire_ai_panel(MainWindow *win);

/// Create the hidden AI dock body on first use and connect its terminal actions.
AiPanel *ensure_ai_panel(MainWindow *win);
