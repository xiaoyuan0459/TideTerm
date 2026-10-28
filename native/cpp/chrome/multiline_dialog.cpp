#include "cpp/clipboard.h"
#include "cpp/chrome/multiline_dialog.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QTimer>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::Str rs(const QByteArray &s) { return rust::Str(s.constData(), size_t(s.size())); }
QString error_text(uint8_t error) {
    switch (error) {
    case 1: return i18n::tr("发送目标已失效，请关闭后重新打开；草稿已保留");
    case 2: return i18n::tr("请输入要发送的文本");
    case 3: return i18n::tr("文本或转换结果超过 64 KiB / 2048 行，请缩小范围");
    case 5: return i18n::tr("输入队列或字符集拒绝发送，已停止；部分目标可能已接收本行");
    default: return i18n::tr("发送选项无效");
    }
}

class MultilineDialog final : public QDialog {
public:
    MultilineDialog(MainWindow *window, PtyView *source, const QString &text, uint8_t paste, int scope,
                    const std::vector<uint32_t> &targets, QWidget *draft_owner)
        : QDialog(window), window_(window), source_(source), draft_owner_(draft_owner),
          explicit_targets_(targets), tab_(source->tab()), paste_(paste) {
        setObjectName(QStringLiteral("multilineEditor"));
        setProperty("sourceTab", tab_);
        setAttribute(Qt::WA_DeleteOnClose);
        setWindowTitle(i18n::tr("多行编辑与发送"));
        resize(760, 610);
        auto *layout = new QVBoxLayout(this);
        auto *options = new QHBoxLayout;
        target_ = new QComboBox(this);
        target_->setObjectName(QStringLiteral("multilineTarget"));
        target_->setAccessibleName(i18n::tr("目标"));
        target_->addItems({i18n::tr("当前会话"), i18n::tr("当前分组中的会话"), i18n::tr("当前窗口全部会话")});
        if (scope == 3) target_->addItem(i18n::tr("选定会话（%1）").arg(targets.size()));
        target_->setCurrentIndex(paste ? 0 : scope);
        target_->setEnabled(!paste);
        eol_ = new QComboBox(this);
        eol_->setObjectName(QStringLiteral("multilineEol"));
        eol_->setAccessibleName(i18n::tr("换行"));
        eol_->addItems({QStringLiteral("CR"), QStringLiteral("LF"), QStringLiteral("CRLF")});
        tabs_ = new QComboBox(this);
        tabs_->setObjectName(QStringLiteral("multilineTabs"));
        tabs_->setAccessibleName(QStringLiteral("Tab"));
        tabs_->addItems({i18n::tr("保留 Tab"), i18n::tr("Tab 替换为 2 空格"),
                         i18n::tr("Tab 替换为 4 空格"), i18n::tr("Tab 替换为 8 空格")});
        options->addWidget(target_, 1); options->addWidget(new QLabel(i18n::tr("换行"), this));
        options->addWidget(eol_); options->addWidget(tabs_); layout->addLayout(options);
        targets_label_ = new QLabel(this); targets_label_->setTextFormat(Qt::PlainText);
        targets_label_->setWordWrap(true); layout->addWidget(targets_label_);
        layout->addWidget(new QLabel(i18n::tr("编辑草稿（取消或停止会保留）"), this));
        editor_ = new QPlainTextEdit(this);
        editor_->setObjectName(QStringLiteral("multilineText"));
        editor_->setAccessibleName(i18n::tr("编辑草稿（取消或停止会保留）"));
        const QString saved = draft_owner->property("multilineDraft").toString();
        editor_->setPlainText(!paste && !saved.isEmpty() ? saved : text);
        accepted_ = editor_->toPlainText();
        layout->addWidget(editor_, 2);
        auto *send_options = new QHBoxLayout;
        append_ = new QCheckBox(i18n::tr("末行追加换行"), this);
        append_->setObjectName(QStringLiteral("multilineAppend"));
        paced_ = new QCheckBox(i18n::tr("逐行发送"), this);
        paced_->setObjectName(QStringLiteral("multilinePaced"));
        interval_ = new QSpinBox(this);
        interval_->setObjectName(QStringLiteral("multilineInterval"));
        interval_->setRange(10, 60000); interval_->setValue(250); interval_->setSuffix(QStringLiteral(" ms"));
        interval_->setAccessibleName(i18n::tr("行间隔"));
        send_options->addWidget(append_); send_options->addWidget(paced_);
        send_options->addWidget(new QLabel(i18n::tr("行间隔"), this)); send_options->addWidget(interval_);
        send_options->addStretch(); layout->addLayout(send_options);
        auto *preview_label = new QLabel(i18n::tr("发送预览（CR / LF / Tab 使用转义显示；整段粘贴沿用应用括号模式，逐行模式直接发送）"), this);
        preview_label->setWordWrap(true); layout->addWidget(preview_label);
        preview_ = new QPlainTextEdit(this);
        preview_->setObjectName(QStringLiteral("multilinePreview")); preview_->setReadOnly(true);
        preview_->setAccessibleName(preview_label->text());
        layout->addWidget(preview_, 1);
        status_ = new QLabel(this); status_->setWordWrap(true);
        status_->setObjectName(QStringLiteral("multilineStatus")); layout->addWidget(status_);
        auto *buttons = new QDialogButtonBox(this);
        send_ = buttons->addButton(i18n::tr("确认发送一次"), QDialogButtonBox::ActionRole);
        send_->setObjectName(QStringLiteral("multilineSend")); send_->setAutoDefault(false);
        stop_ = buttons->addButton(i18n::tr("停止发送"), QDialogButtonBox::ActionRole);
        stop_->setObjectName(QStringLiteral("multilineStop")); stop_->setEnabled(false);
        auto *close = buttons->addButton(QDialogButtonBox::Cancel);
        close->setText(i18n::tr("关闭")); close->setDefault(true);
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
        layout->addWidget(buttons);
        timer_ = new QTimer(this); timer_->setSingleShot(true);
        connect(timer_, &QTimer::timeout, this, [this] { step(); });
        // Poll only while this editor exists: catch a retired source/target before confirmation.
        guard_ = new QTimer(this); guard_->setInterval(100);
        connect(guard_, &QTimer::timeout, this, [this] {
            if (!settled_ && (!source_ || source_->is_closed() || (token_ && !multiline_valid(tab_, token_)))) fail(1);
        });
        connect(target_, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] { capture(); refresh(); });
        connect(editor_, &QPlainTextEdit::textChanged, this, [this] {
            const QString edited = editor_->toPlainText();
            if (edited.toUtf8().size() > (64 << 10)) {
                const QSignalBlocker blocked(editor_); editor_->setPlainText(accepted_);
                send_->setEnabled(false); status_->setText(error_text(3)); return;
            }
            accepted_ = edited; refresh();
        });
        for (auto *combo : {eol_, tabs_}) connect(combo, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] { refresh(); });
        for (auto *check : {append_, paced_}) connect(check, &QCheckBox::toggled, this, [this] { refresh(); });
        connect(send_, &QPushButton::clicked, this, [this] { start(); });
        connect(stop_, &QPushButton::clicked, this, [this] {
            settle(); status_->setText(count_ + QLatin1Char('\n') + i18n::tr("已停止，已排入队列的内容不能撤回；草稿保留"));
        });
        connect(draft_owner, &QObject::destroyed, this, &QDialog::close);
        capture(); refresh(); guard_->start(); editor_->setFocus();
    }
    ~MultilineDialog() override {
        multiline_end(tab_, token_);
        if (draft_owner_) draft_owner_->setProperty("multilineDraft", completed_ ? QString() : editor_->toPlainText());
    }
    void done(int result) override {
        settle();
        if (draft_owner_) draft_owner_->setProperty("multilineDraft", completed_ ? QString() : editor_->toPlainText());
        QDialog::done(result);
    }
    bool owns(QWidget *owner) const { return draft_owner_ == owner; }
private:
    SendOptions options() const {
        static const uint8_t spaces[] = {0, 2, 4, 8};
        return {uint8_t(eol_->currentIndex()), spaces[tabs_->currentIndex()], append_->isChecked(), paced_->isChecked()};
    }
    void capture() {
        multiline_end(tab_, token_);
        const auto ids = target_->currentIndex() == 3 ? explicit_targets_
            : window_->send_target_ids(tab_, target_->currentIndex());
        token_ = multiline_begin(tab_, rust::Slice<const uint32_t>(ids.data(), ids.size()), paste_);
        QStringList names;
        for (size_t i = 0; i < ids.size() && i < 8; ++i) names.push_back(MainWindow::q_title(ids[i]).left(64));
        if (ids.size() > 8) names.push_back(QStringLiteral("…"));
        targets_label_->setText(i18n::tr("固定目标 %1 个：%2").arg(ids.size()).arg(names.join(QStringLiteral(" · "))));
    }
    void refresh() {
        if (running_ || settled_) return;
        const auto text = editor_->toPlainText().toUtf8(); const auto opts = options();
        const auto result = multiline_preview(rs(text), opts);
        preview_->setPlainText(q(result.text));
        send_->setEnabled(result.error == 0 && token_ != 0);
        interval_->setEnabled(paced_->isChecked());
        status_->setText(!token_ ? error_text(1) : result.error ? error_text(result.error)
            : i18n::tr("%1 行 · %2 UTF-8 字节 · 目标已固定；确认前不会发送").arg(result.lines).arg(result.bytes));
    }
    void start() {
        if (running_ || settled_) return;
        const auto text = editor_->toPlainText().toUtf8(); const auto opts = options();
        const auto result = multiline_prepare(tab_, token_, rs(text), opts);
        if (result.error) { fail(result.error); return; }
        running_ = true; send_->setEnabled(false); stop_->setEnabled(true);
        QWidget *controls[] = {editor_, target_, eol_, tabs_, append_, paced_, interval_};
        for (auto *widget : controls) widget->setEnabled(false);
        step();
    }
    void step() {
        if (!running_) return;
        const auto result = multiline_step(tab_, token_);
        if (result.error) { fail(result.error); return; }
        count_ = i18n::tr("已排入队列 %1 / %2 段；不表示远端执行完成").arg(result.sent).arg(result.total);
        status_->setText(count_);
        if (result.sent == result.total) { completed_ = true; settle(); }
        else timer_->start(interval_->value());
    }
    void settle() {
        running_ = false; settled_ = true; timer_->stop(); guard_->stop();
        target_->setEnabled(false);
        multiline_end(tab_, token_); send_->setEnabled(false); stop_->setEnabled(false);
    }
    void fail(uint8_t error) { settle(); status_->setText(count_ + QLatin1Char('\n') + error_text(error)); }
    MainWindow *window_;
    QPointer<PtyView> source_;
    QPointer<QWidget> draft_owner_;
    std::vector<uint32_t> explicit_targets_;
    uint32_t tab_; uint8_t paste_; uint64_t token_ = 0;
    bool running_ = false, settled_ = false, completed_ = false;
    QString count_, accepted_;
    QComboBox *target_, *eol_, *tabs_;
    QPlainTextEdit *editor_, *preview_;
    QCheckBox *append_, *paced_;
    QSpinBox *interval_;
    QLabel *status_, *targets_label_;
    QPushButton *send_, *stop_;
    QTimer *timer_, *guard_;
};
}

void chrome::show_multiline(MainWindow *window, PtyView *source, const QString &text, uint8_t paste, int scope,
                            const std::vector<uint32_t> &targets, QWidget *draft_owner) {
    if (!window || !source || source->is_closed() || !palette_allowed()) return;
    if (!draft_owner) draft_owner = source;
    for (auto *dialog : window->findChildren<QDialog *>(QStringLiteral("multilineEditor"))) {
        if (auto *editor = dynamic_cast<MultilineDialog *>(dialog); editor && editor->owns(draft_owner)) {
            dialog->raise(); dialog->activateWindow(); return;
        }
    }
    if (text.toUtf8().size() > (64 << 10)) { window->note(error_text(3)); return; }
    auto *dialog = new MultilineDialog(window, source, text, paste, scope, targets, draft_owner);
    dialog->show();
}

void chrome::paste_clipboard(PtyView *view) {
    if (!view || view->is_closed() || !palette_allowed()) return;
    const QString text = host_clipboard::read_text();
    auto *window = dynamic_cast<MainWindow *>(view->window());
    if (text.isEmpty()) {
        if (window) window->note(i18n::tr("剪贴板为空或不可用"));
        return;
    }
    if (text.contains(QLatin1Char('\n')) || text.contains(QLatin1Char('\r'))) {
        if (window) show_multiline(window, view, text, 1);
        else term_paste_from_clipboard(view->tab());
    } else {
        term_paste_from_clipboard(view->tab()); view->sync_reading_mode();
    }
    view->update();
}
