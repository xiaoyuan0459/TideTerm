// SendBar —— 发送栏（Phase C ③；2026-09-25 WT 对齐批次 3 重排）。
//
// WT 实测结构（docs/reports/qt-windterm-ui-align-2026-09-25.md §2.1）：
//   底栏 = tab 行（24px）+ 参数条单行（23px）+ 输入区；参数条顺序
//   `发送▾` · `文本(T)` · `Hex(H)` · `换行(L)` · `计数(C)` · `间隔(I)` · `目标(T)`。
// 发送路径真实：文本/Hex 两模式 + CR/LF/CRLF/无 换行 → 活动会话 PTY。
// 多段、定时与目标选择均由本控件驱动；字节仍经 MainWindow 的 Rust 桥发送。
//
// Character pacing uses Qt grapheme boundaries; Hex pacing sends individual bytes.
#pragma once

#include <QLabel>
#include <QWidget>

#include <cstdint>
#include <functional>
#include <string>
#include <utility>
#include <vector>

class QLineEdit;
class QPushButton;
class QComboBox;
class QDoubleSpinBox;
class QSpinBox;
class QTimer;
class QToolButton;
class QVBoxLayout;

class SendBar : public QWidget {
public:
    /// 字节写入器（MainWindow 注入：写活动会话 PTY）与状态反馈。
    using Writer = std::function<bool(const std::string &bytes, bool raw)>;
    using Note = std::function<void(const QString &msg)>;

    explicit SendBar(QWidget *parent = nullptr);

    void set_writer_factory(std::function<Writer(int)> factory, Note n);
    void set_target_picker(std::function<int()> picker) { target_picker_ = std::move(picker); }
    void set_completion_source(std::function<uint32_t()> source);
    void sync_completion_source();
    uint32_t completion_source() const { return completion_source_ ? completion_source_() : UINT32_MAX; }
    void focus_input();
    void set_multiline_opener(std::function<void(const QString &, int)> opener) { multiline_opener_ = std::move(opener); }

    /// 自检辅助：输入框 / 发送次数 / 当前换行模式。
    QLineEdit *input() const { return input_; }
    void send();
    void cancel() { stop_send(); }
    uint32_t sent_count() const { return sent_; }
    QString newline_mode() const;
    void set_hex_mode(bool enabled) { if (mode_select_) mode_select_(enabled ? 1 : 0); }

private:
    bool build_bytes(std::string *out) const;
    void update_byte_count();
    void update_controls();
    void dispatch_cycle();
    bool deliver(const std::string &bytes);
    void finish_cycle();
    void stop_send();
    QWidget *body_ = nullptr;                 // 输入区（折叠目标：输入行 + 多段）
    QLineEdit *input_ = nullptr;
    QLabel *bytes_ = nullptr;                 // N 字节（「数」位）
    QPushButton *send_btn_ = nullptr;
    QToolButton *stop_ = nullptr;
    QToolButton *remove_ = nullptr;
    QToolButton *clear_ = nullptr;
    QVBoxLayout *segment_rows_ = nullptr;
    std::vector<QLineEdit *> segments_;
    QComboBox *target_ = nullptr;
    QDoubleSpinBox *interval_ = nullptr;      // 间隔(I)
    QSpinBox *count_ = nullptr;               // 计数(C)：0 = 无限循环，n = 共发 n 次
    QTimer *timer_ = nullptr;
    QTimer *pacer_ = nullptr;
    QDoubleSpinBox *character_interval_ = nullptr;
    bool character_mode_ = false;
    std::vector<std::string> pending_;
    size_t next_chunk_ = 0;
    size_t pending_bytes_ = 0;
    Writer pending_writer_;
    QString pending_target_;
    bool pending_raw_ = false;
    int mode_ = 0;                            // 0=文本 1=Hex
    std::function<void(int)> mode_select_;
    int newline_ = 0;                         // 0=CR 1=LF 2=CRLF 3=无
    int repeat_count_ = 1;                    // 计数(C) 当前值
    int remaining_ = 0;                       // 定时连发剩余次数（-1 = 无限）
    std::function<Writer(int)> writer_factory_;
    std::function<int()> target_picker_;
    std::function<uint32_t()> completion_source_;
    std::function<void(const QString &, int)> multiline_opener_;
    Note note_;
    uint32_t sent_ = 0;
};
