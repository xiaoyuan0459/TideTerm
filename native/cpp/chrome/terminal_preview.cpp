#include "cpp/clipboard.h"
// One bounded, manually refreshed snapshot per pane. No output subscription or timer.
#include "cpp/chrome/terminal_preview.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QClipboard>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFont>
#include <QLabel>
#include <QPointer>
#include <QPushButton>
#include <QScrollArea>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &value) { return QString::fromUtf8(value.data(), int(value.size())); }

class TerminalPreview final : public QDialog {
public:
    TerminalPreview(MainWindow *window, PtyView *view)
        : QDialog(view), window_(window), view_(view), tab_(view->tab()), uid_(q(term_uid(tab_))) {
        setObjectName(QStringLiteral("terminalPreview"));
        setAttribute(Qt::WA_DeleteOnClose);
        setWindowModality(Qt::NonModal);
        resize(780, 480);
        auto *layout = new QVBoxLayout(this);
        source_ = new QLabel(this);
        source_->setTextFormat(Qt::PlainText);
        source_->setWordWrap(true);
        layout->addWidget(source_);
        text_ = new QLabel(this);
        text_->setObjectName(QStringLiteral("previewText"));
        text_->setTextFormat(Qt::PlainText);
        text_->setAlignment(Qt::AlignLeft | Qt::AlignTop);
        text_->setTextInteractionFlags(Qt::TextSelectableByMouse | Qt::TextSelectableByKeyboard);
        text_->setFocusPolicy(Qt::StrongFocus);
        QFont font(q(term_font_family()));
        font.setPixelSize(int(term_font_px()));
        text_->setFont(font);
        auto *area = new QScrollArea(this);
        area->setWidgetResizable(true);
        area->setWidget(text_);
        layout->addWidget(area, 1);
        status_ = new QLabel(this);
        status_->setObjectName(QStringLiteral("previewStatus"));
        status_->setTextFormat(Qt::PlainText);
        status_->setWordWrap(true);
        layout->addWidget(status_);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
        buttons->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
        copy_ = buttons->addButton(i18n::tr("复制预览"), QDialogButtonBox::ActionRole);
        copy_->setObjectName(QStringLiteral("previewCopy"));
        auto *refresh = buttons->addButton(i18n::tr("刷新"), QDialogButtonBox::ActionRole);
        refresh->setObjectName(QStringLiteral("previewRefresh"));
        layout->addWidget(buttons);
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::close);
        connect(refresh, &QPushButton::clicked, this, [this] { reload(); });
        connect(copy_, &QPushButton::clicked, this, [this] {
            if (!ready()) { unavailable(); return; }
            if (copy_->isEnabled() && !host_clipboard::copy_text(text_->text()))
                status_->setText(i18n::tr("剪贴板为空或不可用"));
        });
    }

    void bind(uint64_t command_id) {
        uid_ = q(term_uid(tab_)); // Explicit reopening may select a new transport in this pane.
        command_id_ = command_id;
        setWindowTitle(command_id ? i18n::tr("命令输出预览") : i18n::tr("选择预览"));
        QString origin = window_->q_title(tab_);
        if (command_id) for (const auto &row : term_commands(tab_)) if (row.id == command_id) {
            origin += QStringLiteral(" · ") + (row.startup ? i18n::tr("连接启动输出") : q(row.title).simplified()); break;
        }
        source_->setText(i18n::tr("来源：%1").arg(origin.left(240)));
        source_->setToolTip(origin);
        reload();
    }

private:
    bool ready() const {
        return window_ && !window_->property("screenLocked").toBool() &&
            view_ && !view_->is_closed() && view_->tab() == tab_ && q(term_uid(tab_)) == uid_;
    }
    void unavailable() {
        text_->clear(); copy_->setEnabled(false);
        status_->setText(i18n::tr("预览来源已失效，请关闭后重新打开"));
    }
    void reload() {
        if (!ready()) { unavailable(); return; }
        QString value;
        bool available = false, truncated = false;
        if (command_id_) {
            const auto content = term_command_output(tab_, command_id_, false);
            available = content.available; truncated = content.truncated; value = q(content.text);
        } else {
            const auto content = term_selection_preview(tab_);
            if (content.exceeds_budget) {
                text_->clear(); copy_->setEnabled(false);
                status_->setText(i18n::tr("选择内容超过 1 MiB，请缩小范围后复制"));
                return;
            }
            available = content.available; truncated = content.truncated; value = q(content.text);
        }
        text_->setText(value); copy_->setEnabled(available && !value.isEmpty());
        status_->setText(!available ? (command_id_ ? i18n::tr("命令位置已失效，请刷新")
                                                  : i18n::tr("没有已选内容可预览"))
            : truncated ? i18n::tr("预览已截断到 64 KiB 或 1024 行；复制仅包含显示内容")
                        : i18n::tr("文本快照；点击刷新更新，复制仅包含显示内容"));
    }
    QPointer<MainWindow> window_;
    QPointer<PtyView> view_;
    uint32_t tab_;
    QString uid_;
    uint64_t command_id_ = 0;
    QLabel *source_ = nullptr;
    QLabel *text_ = nullptr;
    QLabel *status_ = nullptr;
    QPushButton *copy_ = nullptr;
};
}

void chrome::show_terminal_preview(MainWindow *window, PtyView *view, uint64_t command_id) {
    if (!window || window->property("screenLocked").toBool() || !view || view->is_closed()) return;
    TerminalPreview *preview = nullptr;
    for (auto *child : view->findChildren<QDialog *>(QString(), Qt::FindDirectChildrenOnly))
        if (auto *existing = dynamic_cast<TerminalPreview *>(child)) { preview = existing; break; }
    if (!preview) preview = new TerminalPreview(window, view);
    preview->bind(command_id);
    preview->show(); preview->raise(); preview->activateWindow();
}
