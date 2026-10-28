#include "cpp/chrome/send_completion.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/i18n.h"
#include <QComboBox>
#include <QAction>
#include <QEvent>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QPointer>
#include <QSignalBlocker>
#include <QTimer>
#include <QToolButton>
#include <QToolTip>
#include <QVBoxLayout>
#include <algorithm>
#include <vector>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
class QuickBar final : public QWidget {
public:
    QuickBar(QWidget *owner, QLineEdit *input, std::function<uint32_t()> source, std::function<bool()> enabled)
        : QWidget(owner), input_(input), source_(std::move(source)), enabled_(std::move(enabled)) {
        setObjectName(QStringLiteral("sendQuickBar"));
        auto *row = new QHBoxLayout(this); row->setContentsMargins(8, 0, 8, 0); row->setSpacing(4);
        auto *label = new QLabel(i18n::tr("用户命令"), this); row->addWidget(label);
        groups_ = new QComboBox(this); groups_->setObjectName(QStringLiteral("quickCommandGroup"));
        groups_->setMaximumWidth(180); row->addWidget(groups_);
        buttons_ = new QHBoxLayout; buttons_->setSpacing(4); row->addLayout(buttons_, 1);
        input_->installEventFilter(this);
        connect(groups_, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] { render(); });
    }
    void refresh() {
        auto catalog = command_catalog();
        if (revision_ == catalog.revision && error_ == catalog.error) return;
        revision_ = catalog.revision; error_ = catalog.error;
        const QString selected = groups_->currentData().toString(); const bool all = groups_->currentIndex() <= 0;
        const QSignalBlocker block(groups_); groups_->clear(); groups_->addItem(i18n::tr("全部分组"));
        QStringList groups;
        for (const auto &entry : catalog.entries) if (!groups.contains(q(entry.group))) groups << q(entry.group);
        groups.sort();
        for (const auto &group : groups) groups_->addItem(group.isEmpty() ? i18n::tr("未分组") : group, group);
        const int at = groups_->findData(selected); groups_->setCurrentIndex(!all && at >= 0 ? at : 0);
        render();
    }
protected:
    void showEvent(QShowEvent *event) override { QWidget::showEvent(event); refresh(); }
    bool eventFilter(QObject *, QEvent *event) override {
        if (event->type() == QEvent::FocusIn) refresh();
        return false;
    }
private:
    void choose(uint32_t id, uint64_t revision) {
        const auto context = completion_context(source_());
        if (context.catalog_revision != revision) {
            refresh(); QToolTip::showText(mapToGlobal(QPoint()), i18n::tr("命令目录已更新，请重新选择。"), this); return;
        }
        const QPointer<QuickBar> guard(this);
        const auto enabled = enabled_; const auto source = source_;
        chrome::fill_user_command(input_->window(), input_, context, id, [guard, enabled, source, &context] {
            return guard && enabled() && guard->isVisible() && source() == context.source;
        });
    }
    void render() {
        while (auto *item = buttons_->takeAt(0)) { if (auto *widget = item->widget()) { widget->hide(); widget->deleteLater(); } delete item; }
        auto catalog = command_catalog();
        std::vector<const UserCommand *> entries;
        for (const auto &entry : catalog.entries) if (groups_->currentIndex() <= 0 || q(entry.group) == groups_->currentData().toString()) entries.push_back(&entry);
        std::sort(entries.begin(), entries.end(), [](const auto *a, const auto *b) { return q(a->name) < q(b->name); });
        QToolButton *more = nullptr; QMenu *menu = nullptr;
        for (size_t i = 0; i < entries.size(); ++i) {
            const auto &entry = *entries[i]; const uint32_t id = entry.id; const uint64_t revision = catalog.revision;
            const QString title = q(entry.name).left(32);
            const QString tip = q(entry.command) + QStringLiteral("\n") + i18n::tr("仅填入草稿，发送需再次确认。");
            if (i < 8) {
                auto *button = new QToolButton(this); button->setText(title); button->setToolTip(tip);
                button->setToolButtonStyle(Qt::ToolButtonTextOnly);
                button->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed); button->setMinimumWidth(32); button->setMaximumWidth(120);
                button->setAccessibleName(q(entry.name)); button->setObjectName(QStringLiteral("quickCommand_%1").arg(id));
                buttons_->addWidget(button); connect(button, &QToolButton::clicked, this, [this,id,revision] { choose(id,revision); });
            } else {
                if (!more) { more = new QToolButton(this); more->setObjectName(QStringLiteral("quickCommandMore")); more->setText(i18n::tr("更多…")); menu = new QMenu(more); chrome::style_menu(menu); more->setMenu(menu); more->setPopupMode(QToolButton::InstantPopup); buttons_->addWidget(more); }
                auto *action = menu->addAction(q(entry.name)); action->setToolTip(tip);
                connect(action, &QAction::triggered, this, [this,id,revision] { choose(id,revision); });
            }
        }
        if (entries.empty()) buttons_->addWidget(new QLabel(catalog.error ? i18n::tr("用户命令不可用，请打开管理查看。") : i18n::tr("暂无命令，可通过输入框右侧按钮添加。"), this));
        buttons_->addStretch(1);
    }
    QLineEdit *input_;
    QComboBox *groups_;
    QHBoxLayout *buttons_;
    std::function<uint32_t()> source_;
    std::function<bool()> enabled_;
    uint64_t revision_ = 0;
    uint8_t error_ = 0;
};
}
namespace chrome {
void install_send_quickbar(QWidget *owner, QVBoxLayout *layout, QLineEdit *input,
                          std::function<uint32_t()> source, std::function<bool()> enabled) {
    layout->addWidget(new QuickBar(owner, input, std::move(source), std::move(enabled)));
}
void refresh_send_quickbar(QWidget *owner) {
    if (auto *bar = dynamic_cast<QuickBar *>(owner->findChild<QWidget *>(QStringLiteral("sendQuickBar")))) bar->refresh();
}
}
