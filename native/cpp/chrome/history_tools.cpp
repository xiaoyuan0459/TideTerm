#include "cpp/chrome/history_tools.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPointer>
#include <QPushButton>
#include <QScrollArea>
#include <QSpinBox>
#include <QTimer>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }

class HistoryTools final : public QDialog {
public:
    explicit HistoryTools(PtyView *view) : QDialog(view), view_(view), source_(view->tab()),
        generation_(term_history_state(source_).generation) {
        setObjectName(QStringLiteral("historyTools"));
        setWindowTitle(i18n::tr("历史导出与浏览"));
        setAttribute(Qt::WA_DeleteOnClose);
        resize(800, 540);
        auto *layout = new QVBoxLayout(this);
        auto *info = new QLabel(i18n::tr("仅导出当前保留历史与屏幕，已淘汰历史不可恢复。导出为 UTF-8 纯文本，临时文件在关闭或浏览器退出后清理。"), this);
        info->setWordWrap(true); layout->addWidget(info);
        auto *settings = new QHBoxLayout;
        settings->addWidget(new QLabel(i18n::tr("磁盘上限（MiB）"), this));
        limit_ = new QSpinBox(this); limit_->setObjectName(QStringLiteral("historyDiskLimit"));
        limit_->setRange(4, 2048); limit_->setValue(512); settings->addWidget(limit_);
        map_ = new QCheckBox(i18n::tr("大文件只读 mmap（200 MiB 起）"), this); settings->addWidget(map_);
        layout->addLayout(settings);
        auto *actions = new QHBoxLayout;
        save_ = new QPushButton(i18n::tr("导出文件…"), this);
        save_->setObjectName(QStringLiteral("historySave")); actions->addWidget(save_);
        browse_ = new QPushButton(i18n::tr("临时浏览"), this);
        browse_->setObjectName(QStringLiteral("historyBrowse")); actions->addWidget(browse_);
        cancel_ = new QPushButton(i18n::tr("取消任务并清理"), this); actions->addWidget(cancel_);
        layout->addLayout(actions);
        auto *navigation = new QHBoxLayout;
        previous_ = new QPushButton(i18n::tr("上一页"), this); navigation->addWidget(previous_);
        page_ = new QSpinBox(this); page_->setObjectName(QStringLiteral("historyPage"));
        page_->setRange(1, 1); navigation->addWidget(page_);
        next_ = new QPushButton(i18n::tr("下一页"), this); navigation->addWidget(next_);
        pager_ = new QComboBox(this); pager_->addItems({QStringLiteral("less"), QStringLiteral("nvim")}); navigation->addWidget(pager_);
        external_ = new QPushButton(i18n::tr("外部只读浏览…"), this); navigation->addWidget(external_);
        external_->setToolTip(i18n::tr("Windows 将打开独立控制台；Linux/macOS 需要 xterm。浏览器退出后清理临时历史。"));
        layout->addLayout(navigation);
        text_ = new QLabel(this); text_->setObjectName(QStringLiteral("historyPageText"));
        text_->setTextFormat(Qt::PlainText); text_->setAlignment(Qt::AlignLeft | Qt::AlignTop);
        text_->setTextInteractionFlags(Qt::TextSelectableByMouse | Qt::TextSelectableByKeyboard);
        text_->setFocusPolicy(Qt::StrongFocus);
        auto *area = new QScrollArea(this); area->setWidgetResizable(true); area->setWidget(text_); layout->addWidget(area, 1);
        status_ = new QLabel(this); status_->setObjectName(QStringLiteral("historyStatus"));
        status_->setTextFormat(Qt::PlainText); status_->setWordWrap(true); layout->addWidget(status_);
        auto *close = new QDialogButtonBox(QDialogButtonBox::Close, this); layout->addWidget(close);
        close->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
        connect(close, &QDialogButtonBox::rejected, this, &QDialog::close);
        connect(save_, &QPushButton::clicked, this, [this] {
            const auto target = QFileDialog::getSaveFileName(this, i18n::tr("导出文件…"), QStringLiteral("history.txt"), QStringLiteral("Text (*.txt)"));
            if (!target.isEmpty()) start(target);
        });
        connect(browse_, &QPushButton::clicked, this, [this] { start(QString()); });
        connect(cancel_, &QPushButton::clicked, this, [this] {
            history_export_close(source_); text_->clear(); last_state_ = 0;
            status_->setText(i18n::tr("已取消并清理临时历史")); controls(0);
        });
        connect(page_, &QSpinBox::valueChanged, this, [this] { load_page(); });
        connect(previous_, &QPushButton::clicked, this, [this] { page_->setValue(page_->value() - 1); });
        connect(next_, &QPushButton::clicked, this, [this] { page_->setValue(page_->value() + 1); });
        connect(external_, &QPushButton::clicked, this, [this] {
            const auto executable = QFileDialog::getOpenFileName(this, i18n::tr("选择 less 或 nvim 可执行文件"));
            if (executable.isEmpty()) return;
            const auto error = history_export_pager(source_, executable.toUtf8().toStdString(), uint8_t(pager_->currentIndex() + 1));
            if (!error.empty()) status_->setText(i18n::tr("操作失败：%1").arg(q(error)));
            else { text_->clear(); refresh(); }
        });
        auto *timer = new QTimer(this); timer->setInterval(100);
        connect(timer, &QTimer::timeout, this, [this] { refresh(); }); timer->start();
        controls(0);
    }
    ~HistoryTools() override { history_export_close(source_); }
private:
    void controls(uint8_t state) {
        const bool busy = state == 1 || state == 3;
        save_->setEnabled(!busy); browse_->setEnabled(!busy); map_->setEnabled(!busy); limit_->setEnabled(!busy);
        cancel_->setEnabled(state != 0 && state != 4 && state != 6);
        page_->setEnabled(state == 2); previous_->setEnabled(state == 2 && page_->value() > 1);
        next_->setEnabled(state == 2 && page_->value() < page_->maximum()); external_->setEnabled(state == 2);
    }
    void start(const QString &target) {
        if (!view_ || view_->is_closed()) { close(); return; }
        const auto error = history_export_start(source_, target.toUtf8().toStdString(), uint32_t(limit_->value()), map_->isChecked());
        if (!error.empty()) { status_->setText(i18n::tr("操作失败：%1").arg(q(error))); return; }
        last_state_ = 0; text_->clear(); refresh();
    }
    void load_page() {
        const auto value = history_export_page(source_, uint32_t(page_->value() - 1));
        text_->setText(value.available ? q(value.text) : QString());
        controls(history_export_poll(source_).state);
    }
    void refresh() {
        const auto source = term_history_state(source_);
        if (!view_ || view_->is_closed() || !source.available || source.generation != generation_) { close(); return; }
        const auto state = history_export_poll(source_);
        if (state.state == 1) status_->setText(i18n::tr("正在导出：%1 / %2 行").arg(state.completed).arg(state.rows));
        else if (state.state == 2) {
            status_->setText(i18n::tr("快照 %1 字节；每页 64 KiB；读取方式：%2").arg(qulonglong(state.bytes)).arg(state.mapped ? QStringLiteral("mmap") : i18n::tr("流式分页")));
            if (last_state_ != 2) {
                page_->setRange(1, int(qMax<uint64_t>(1, (state.bytes + 65535) / 65536)));
                page_->setValue(1); load_page();
            }
        } else if (state.state == 3) status_->setText(i18n::tr("外部浏览器运行中，退出后清理临时文件"));
        else if (state.state == 4) status_->setText(i18n::tr("操作已完成"));
        else if (state.state == 5) status_->setText(i18n::tr("操作失败：%1").arg(q(state.error)));
        else if (state.state == 6) status_->setText(i18n::tr("已取消并清理临时历史"));
        controls(state.state); last_state_ = state.state;
    }
    QPointer<PtyView> view_;
    uint32_t source_, generation_;
    uint8_t last_state_ = 0;
    QSpinBox *limit_, *page_;
    QCheckBox *map_;
    QComboBox *pager_;
    QPushButton *save_, *browse_, *cancel_, *previous_, *next_, *external_;
    QLabel *text_, *status_;
};
}

void chrome::show_history_tools(PtyView *view) {
    if (!view || view->is_closed() || !term_history_state(view->tab()).available) return;
    for (auto *child : view->findChildren<QDialog *>(QString(), Qt::FindDirectChildrenOnly)) {
        if (auto *existing = dynamic_cast<HistoryTools *>(child)) { existing->show(); existing->raise(); existing->activateWindow(); return; }
    }
    auto *dialog = new HistoryTools(view); dialog->show(); dialog->raise(); dialog->activateWindow();
}
