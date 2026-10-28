// Compact sender toolbar and per-session drafts; all controls use the real PTY writer.

#include "cpp/chrome/send_bar.h"
#include <QVariant>
#include "cpp/bridge_shim.h"
#include "cpp/chrome/send_completion.h"
#include "cpp/i18n.h"
#include "cpp/chrome/ai_panel.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/prefs_dialog.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/main_window.h"

#include <QHBoxLayout>
#include <QCheckBox>
#include <QComboBox>
#include <QDockWidget>
#include <QDoubleSpinBox>
#include <QFrame>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>
#include <QToolButton>
#include <QTimer>
#include <QVBoxLayout>
#include <QMenu>
#include <QActionGroup>
#include <QWidgetAction>
#include "cpp/chrome/menu_style.h"

#include <algorithm>

namespace {

/// 参数条统一控件高（条高 23 内居中；分段容器/数字框/下拉框同一高度 ⇒ 基线对齐）。
constexpr int kCtlHeight = 19;

QLabel *dim_label(const QString &text, const QString &tip, QWidget *parent) {
    auto *l = new QLabel(text, parent);
    l->setObjectName(QStringLiteral("mutedText"));
    l->setToolTip(tip);
    return l;
}

} // namespace

SendBar::SendBar(QWidget *parent) : QWidget(parent) {
    setObjectName(QStringLiteral("sendBar"));
    setAttribute(Qt::WA_StyledBackground, true);
    auto *root = new QVBoxLayout(this);
    root->setContentsMargins(0, 0, 0, 0);
    root->setSpacing(0);

    // The movable dock owns the single title row; options stay in this strip.
    // ── 参数条（单行，WT 顺序）：发送▾ · 文本(T)|Hex(H) · 换行(L) · 计数(C) · 间隔(I) · 目标(T) ──
    auto *strip = new QWidget(this);
    strip->setObjectName(QStringLiteral("sendParams"));
    strip->setFixedHeight(chrome::metrics::kSendParamRowHeight);
    auto *sl = new QHBoxLayout(strip);
    sl->setContentsMargins(8, 0, 8, 0);
    sl->setSpacing(6);

    // 定时器先于两个数值控件建：`setValue(...)` 会同步触发 valueChanged，回调里要用到它。
    timer_ = new QTimer(this);
    timer_->setSingleShot(true);
    connect(timer_, &QTimer::timeout, this, [this] { dispatch_cycle(); });
    pacer_ = new QTimer(this);
    connect(pacer_, &QTimer::timeout, this, [this] {
        if (next_chunk_ < pending_.size() && !deliver(pending_[next_chunk_++])) return;
        if (next_chunk_ == pending_.size()) finish_cycle();
    });

    send_btn_ = new QPushButton(strip);
    chrome::set_button_icon(send_btn_, QStringLiteral("play"), 12, chrome::IconInk::Success);
    send_btn_->setObjectName(QStringLiteral("sendStart"));
    send_btn_->setToolTip(i18n::tr("发送"));
    send_btn_->setAccessibleName(send_btn_->toolTip());
    send_btn_->setFixedSize(20, 20);
    connect(send_btn_, &QPushButton::clicked, this, [this] { send(); });
    sl->addWidget(send_btn_);
    auto tool = [strip, sl](const QString &name, const QString &tip) {
        auto *button = new QToolButton(strip);
        button->setObjectName(QStringLiteral("sendCtl"));
        chrome::set_button_icon(button, name, 12);
        button->setToolTip(tip);
        button->setAccessibleName(tip);
        button->setFixedSize(20, 20);
        sl->addWidget(button);
        return button;
    };
    stop_ = tool(QStringLiteral("stop"), i18n::tr("停止发送"));
    connect(stop_, &QToolButton::clicked, this, [this] {
        stop_send();
    });
    auto *add_seg = tool(QStringLiteral("add"), i18n::tr("添加发送段"));
    auto *multiline = tool(QStringLiteral("edit"), i18n::tr("多行编辑与发送"));
    multiline->setObjectName(QStringLiteral("sendMultiline"));
    connect(multiline, &QToolButton::clicked, this, [this] {
        if (multiline_opener_) multiline_opener_(input_->text(), target_->currentIndex());
    });
    remove_ = tool(QStringLiteral("remove"), i18n::tr("删除最后一个发送段"));
    clear_ = tool(QStringLiteral("clear_input"), i18n::tr("清空发送内容"));
    auto *mode = new QWidget(strip);
    auto *modes = new QHBoxLayout(mode);
    modes->setContentsMargins(4, 0, 4, 0);
    modes->setSpacing(8);
    auto *text = new QRadioButton(i18n::tr("文本(T)"), mode);
    auto *hex = new QRadioButton(QStringLiteral("Hex(H)"), mode);
    modes->addWidget(text);
    modes->addWidget(hex);
    connect(text, &QRadioButton::toggled, this, [this](bool on) { if (on) mode_ = 0; });
    connect(hex, &QRadioButton::toggled, this, [this](bool on) { if (on) mode_ = 1; });
    mode_select_ = [text, hex](int i) { (i == 1 ? hex : text)->setChecked(true); };
    text->setChecked(true);
    sl->addWidget(mode);
    sl->addWidget(dim_label(i18n::tr("换行"), QString(), strip));
    auto *newline = new QComboBox(strip);
    newline->setObjectName(QStringLiteral("sendNewline"));
    newline->addItems({QStringLiteral("CR"), QStringLiteral("LF"),
                       QStringLiteral("CRLF"), i18n::tr("无")});
    newline->setFixedSize(64, kCtlHeight);
    connect(newline, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this](int i) { newline_ = i; });
    sl->addWidget(newline);

    // 计数：1 = 单次；n>1 = 共发 n 次（按间隔）；0 = 无限循环（沿用原「循环」语义）。
    sl->addWidget(dim_label(i18n::tr("计数"), i18n::tr("0 = 无限循环，n = 共发 n 次"),
                            strip));
    count_ = new QSpinBox(strip);
    count_->setRange(0, 9999);
    count_->setValue(1);
    count_->setFixedHeight(kCtlHeight);
    connect(count_, qOverload<int>(&QSpinBox::valueChanged), this, [this](int v) {
        repeat_count_ = v;
    });
    sl->addWidget(count_);

    sl->addWidget(dim_label(i18n::tr("间隔"), QString(), strip));
    interval_ = new QDoubleSpinBox(strip);
    interval_->setRange(0.1, 3600.0);
    interval_->setDecimals(2);
    interval_->setSingleStep(0.1);
    interval_->setSuffix(QStringLiteral("s"));
    interval_->setValue(1.00);
    interval_->setFixedHeight(kCtlHeight);
    connect(interval_, qOverload<double>(&QDoubleSpinBox::valueChanged), this,
            [this](double seconds) {
                if (timer_->isActive()) timer_->start(int(seconds * 1000));
            });
    sl->addWidget(interval_);

    sl->addWidget(dim_label(i18n::tr("目标"), QString(), strip));
    target_ = new QComboBox(strip);
    target_->addItems({i18n::tr("当前会话"), i18n::tr("当前分组中的会话"),
                       i18n::tr("所有会话"), i18n::tr("选定会话（%1）").arg(0)});
    target_->setObjectName(QStringLiteral("sendTarget"));
    target_->setFixedHeight(kCtlHeight);
    sl->addWidget(target_);
    auto *pick = new QToolButton(strip);
    pick->setObjectName(QStringLiteral("sendPickTargets"));
    chrome::set_button_icon(pick, QStringLiteral("select_all"), 12);
    pick->setToolTip(i18n::tr("只向勾选的本窗口会话发送"));
    pick->setAccessibleName(i18n::tr("选择发送目标"));
    pick->setFixedSize(20, 20);
    sl->addWidget(pick);
    connect(pick, &QToolButton::clicked, this, [this] {
        if (!target_picker_) return;
        const int count = target_picker_();
        if (count < 0) return;
        target_->setItemText(3, i18n::tr("选定会话（%1）").arg(count));
        target_->setCurrentIndex(3);
    });
    sl->addStretch(1);
    auto *more = new QToolButton(strip);
    more->setObjectName(QStringLiteral("sendCtl"));
    chrome::set_button_icon(more, QStringLiteral("settings"), 12);
    more->setToolTip(i18n::tr("发送设置"));
    more->setFixedSize(20, 20);
    auto *toggle = new QToolButton(strip);
    toggle->setObjectName(QStringLiteral("sendCtl"));
    chrome::set_button_icon(toggle, QStringLiteral("fold"), 12);
    toggle->setToolTip(i18n::tr("折叠/展开发送栏"));
    toggle->setFixedSize(20, 20);
    sl->addWidget(more);
    sl->addWidget(toggle);
    root->addWidget(strip);

    // ── 输入区（折叠目标）：输入行 + 多段 ──
    body_ = new QWidget(this);
    auto *bl = new QVBoxLayout(body_);
    bl->setContentsMargins(0, 0, 0, 0);
    bl->setSpacing(2);

    auto *row = new QWidget(body_);
    auto *rl = new QHBoxLayout(row);
    rl->setContentsMargins(8, 2, 8, 2);
    rl->setSpacing(6);
    input_ = new QLineEdit(row);
    // 命令行的等宽感不值得那 40 MB：占位符就带中文，挂 {Consolas, monospace} 会让 Qt 建出
    // 整套回退引擎链。真要等宽只能挂**单个**自覆盖汉字的族名（如 SimSun），这里用继承脸。
    input_->setPlaceholderText(i18n::tr("输入要发送的内容…"));
    input_->setObjectName(QStringLiteral("sendInput"));
    // 清除走行内 ⓧ（原生 clear button）；不再另设 ×/∨ 重复钮（2026-09-28 排布重做）。
    input_->setClearButtonEnabled(true);
    bytes_ = new QLabel(i18n::tr("0 字节"), row);
    bytes_->setObjectName(QStringLiteral("mutedText"));
    connect(input_, &QLineEdit::textChanged, this, [this] { update_byte_count(); });
    connect(input_, &QLineEdit::returnPressed, this, [this] { send(); });
    rl->addWidget(input_, 1);
    chrome::install_send_completion(this, rl, input_,
        [this] { return completion_source_ ? completion_source_() : UINT32_MAX; },
        [this] { return mode_ == 0 && !timer_->isActive() && !pacer_->isActive(); });
    rl->addWidget(bytes_);
    chrome::install_send_quickbar(this, bl, input_,
        [this] { return completion_source_ ? completion_source_() : UINT32_MAX; },
        [this] { return mode_ == 0 && !timer_->isActive() && !pacer_->isActive(); });
    bl->addWidget(row);
    segment_rows_ = new QVBoxLayout;
    segment_rows_->setContentsMargins(8, 0, 8, 0);
    segment_rows_->setSpacing(2);
    bl->addLayout(segment_rows_);
    bl->addStretch(1);
    connect(add_seg, &QToolButton::clicked, this, [this] {
        if (segments_.size() >= 8) { if (note_) note_(i18n::tr("最多 9 个发送段")); return; }
        auto *seg_row = new QWidget(body_);
        auto *layout = new QHBoxLayout(seg_row);
        layout->setContentsMargins(0, 0, 0, 0);
        auto *entry = new QLineEdit(seg_row);
        entry->setPlaceholderText(i18n::tr("发送段 %1").arg(segments_.size() + 2));
        auto *remove = new QToolButton(seg_row);
        chrome::set_button_icon(remove, QStringLiteral("close"), 12);
        remove->setFixedSize(20, 20);
        remove->setToolTip(i18n::tr("删除发送段"));
        layout->addWidget(entry, 1);
        layout->addWidget(remove);
        segment_rows_->addWidget(seg_row);
        segments_.push_back(entry);
        connect(entry, &QLineEdit::textChanged, this, [this] { update_byte_count(); });
        connect(remove, &QToolButton::clicked, this, [this, seg_row, entry] {
            segments_.erase(std::remove(segments_.begin(), segments_.end(), entry),
                            segments_.end());
            seg_row->deleteLater();
            update_byte_count();
        });
        entry->setFocus();
        update_controls();
    });
    connect(remove_, &QToolButton::clicked, this, [this] {
        if (segments_.empty()) return;
        QLineEdit *entry = segments_.back();
        segments_.pop_back();
        entry->parentWidget()->deleteLater();
        update_byte_count();
    });
    connect(clear_, &QToolButton::clicked, this, [this] {
        input_->clear();
        for (QLineEdit *entry : segments_) entry->clear();
    });
    root->addWidget(body_, 1);

    connect(toggle, &QToolButton::clicked, this, [this, toggle] {
        const bool expand = !body_->isVisible();
        body_->setVisible(expand);
        chrome::set_button_icon(toggle, expand ? QStringLiteral("fold") : QStringLiteral("down"), 12);
    });
    auto *send_settings = new QMenu(more);
    chrome::style_menu(send_settings);
    auto *pacing = new QActionGroup(send_settings);
    pacing->setExclusive(true);
    for (bool characters : {false, true}) {
        auto *action = send_settings->addAction(i18n::tr(characters ? "逐字符发送" : "按行发送"));
        action->setObjectName(characters ? QStringLiteral("sendCharacters") : QStringLiteral("sendLines"));
        action->setCheckable(true);
        action->setChecked(!characters);
        pacing->addAction(action);
        connect(action, &QAction::triggered, this, [this, characters] { character_mode_ = characters; });
    }
    auto *interval_row = new QWidget(send_settings);
    auto *interval_layout = new QHBoxLayout(interval_row);
    interval_layout->addWidget(new QLabel(i18n::tr("字符间隔"), interval_row));
    character_interval_ = new QDoubleSpinBox(interval_row);
    character_interval_->setObjectName(QStringLiteral("sendCharacterInterval"));
    character_interval_->setRange(0.001, 10.0);
    character_interval_->setDecimals(3);
    character_interval_->setSingleStep(0.01);
    character_interval_->setValue(0.01);
    character_interval_->setSuffix(QStringLiteral("s"));
    interval_layout->addWidget(character_interval_);
    auto *interval_action = new QWidgetAction(send_settings);
    interval_action->setDefaultWidget(interval_row);
    send_settings->addAction(interval_action);
    more->setMenu(send_settings);
    more->setPopupMode(QToolButton::InstantPopup);
    update_controls();
}

void SendBar::set_writer_factory(std::function<Writer(int)> factory, Note n) {
    stop_send();
    writer_factory_ = std::move(factory);
    note_ = std::move(n);
}

void SendBar::focus_input() {
    body_->show();
    input_->setFocus();
}
void SendBar::set_completion_source(std::function<uint32_t()> source) {
    completion_source_ = std::move(source); sync_completion_source();
}
void SendBar::sync_completion_source() {
    const auto source = completion_source();
    if (property("completionSource").toUInt() != source) setProperty("completionSource", source);
    const auto revision = QVariant::fromValue(qulonglong(completion_sources_revision(source)));
    if (property("completionSettingsRevision") != revision) setProperty("completionSettingsRevision", revision);
}

QString SendBar::newline_mode() const {
    static const char *k_names[] = {"CR", "LF", "CRLF", "无"};
    return QString::fromUtf8(k_names[newline_]);
}

void SendBar::update_byte_count() {
    int count = input_->text().toUtf8().size();
    for (QLineEdit *segment : segments_) count += segment->text().toUtf8().size();
    bytes_->setText(QString::number(count) + i18n::tr(" 字节"));
    update_controls();
}

void SendBar::update_controls() {
    const bool sending = timer_->isActive() || pacer_->isActive();
    stop_->setEnabled(sending);
    send_btn_->setEnabled(!sending);
    remove_->setEnabled(!segments_.empty());
    bool has_text = !input_->text().isEmpty();
    for (QLineEdit *entry : segments_) has_text |= !entry->text().isEmpty();
    clear_->setEnabled(has_text);
}
