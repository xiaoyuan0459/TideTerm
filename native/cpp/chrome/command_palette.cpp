#include "cpp/chrome/command_palette.h"
#include "cpp/chrome/palette_model.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/menu_actions.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QAction>
#include <QComboBox>
#include <QDialog>
#include <QHeaderView>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSignalBlocker>
#include <QTreeWidget>
#include <QVBoxLayout>
#include <algorithm>
#include <array>
#include <functional>

namespace {
using chrome::PaletteMode; using chrome::PaletteKind; using chrome::PaletteItem;
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::Str rs(const QByteArray &s) { return rust::Str(s.constData(), size_t(s.size())); }
QString prefix(PaletteMode mode) {
    switch (mode) { case PaletteMode::Lines: return QStringLiteral(":"); case PaletteMode::Symbols: return QStringLiteral("@"); case PaletteMode::Sessions: return QStringLiteral("tm "); default: return QStringLiteral(">"); }
}
struct Query { PaletteMode mode; QString body; };
Query parse(QString value) {
    value = value.trimmed();
    if (value.startsWith(QLatin1Char(':'))) return {PaletteMode::Lines, value.mid(1).trimmed()};
    if (value.startsWith(QLatin1Char('@'))) return {PaletteMode::Symbols, value.mid(1).trimmed()};
    if (value.left(3).compare(QStringLiteral("tm "), Qt::CaseInsensitive) == 0 || value.compare(QStringLiteral("tm"), Qt::CaseInsensitive) == 0) return {PaletteMode::Sessions, value.mid(2).trimmed()};
    return {PaletteMode::Actions, (value.startsWith(QLatin1Char('>')) ? value.mid(1) : value).trimmed()};
}
QString line_error(uint8_t code) {
    switch (code) {
    case 1: return i18n::tr("行定位仅适用于终端主屏");
    case 2: return i18n::tr("请输入正整数行号，或 +N / -N 相对偏移");
    case 3: return i18n::tr("行号超出保留范围，请修改输入");
    case 5: return i18n::tr("锁定期间不能使用命令面板");
    case 8: return i18n::tr("会话目录读取失败；可切换已打开的会话或刷新重试");
    default: return i18n::tr("来源已变化或目标已失效，请刷新后重试");
    }
}

class PaletteDialog final : public QDialog {
public:
    PaletteDialog(MainWindow *window, const QString &initial) : QDialog(window), window_(window) {
        setObjectName(QStringLiteral("commandPalette")); setWindowTitle(i18n::tr("命令面板")); resize(900, 570);
        auto *layout = new QVBoxLayout(this); auto *entry = new QHBoxLayout();
        modes_ = new QComboBox(this); modes_->setObjectName(QStringLiteral("paletteMode"));
        modes_->addItems({i18n::tr("> 动作"), i18n::tr(": 行定位"), i18n::tr("@ 命令符号"), i18n::tr("tm 会话")});
        query_ = new QLineEdit(this); query_->setObjectName(QStringLiteral("paletteQuery")); query_->setMaxLength(256);
        query_->setPlaceholderText(i18n::tr("输入名称、动作 ID，或切换 : / @ / tm 模式")); query_->setAccessibleName(i18n::tr("筛选或输入目标"));
        entry->addWidget(modes_); entry->addWidget(query_, 1); layout->addLayout(entry);
        tree_ = new QTreeWidget(this); tree_->setObjectName(QStringLiteral("paletteItems")); tree_->setRootIsDecorated(false); tree_->setUniformRowHeights(true);
        tree_->setHeaderLabels({i18n::tr("项目"), i18n::tr("位置或类型"), i18n::tr("快捷键")}); tree_->header()->setStretchLastSection(false);
        tree_->header()->setSectionResizeMode(0, QHeaderView::Stretch); tree_->header()->setSectionResizeMode(1, QHeaderView::Interactive); tree_->setColumnWidth(1, 250); tree_->header()->setSectionResizeMode(2, QHeaderView::ResizeToContents);
        layout->addWidget(tree_, 1);
        status_ = new QLabel(this); status_->setObjectName(QStringLiteral("paletteStatus")); status_->setTextFormat(Qt::PlainText); status_->setWordWrap(true); layout->addWidget(status_);
        auto *hint = new QLabel(i18n::tr("↑↓ 选择，Enter 确认，Esc 取消；输入只筛选，刷新获取最新候选。"), this); hint->setWordWrap(true); layout->addWidget(hint);
        auto *buttons = new QHBoxLayout();
        refresh_ = new QPushButton(i18n::tr("刷新"), this); refresh_->setObjectName(QStringLiteral("paletteRefresh")); buttons->addWidget(refresh_); buttons->addStretch();
        auto *cancel = new QPushButton(i18n::tr("取消"), this); buttons->addWidget(cancel);
        go_ = new QPushButton(i18n::tr("执行或定位"), this); go_->setObjectName(QStringLiteral("paletteGo")); go_->setDefault(true); buttons->addWidget(go_); layout->addLayout(buttons);
        query_->installEventFilter(this); tree_->installEventFilter(this);
        connect(query_, &QLineEdit::textChanged, this, [this] { rebuild(false); });
        connect(modes_, &QComboBox::activated, this, [this](int index) { switch_mode(PaletteMode(index)); });
        connect(tree_, &QTreeWidget::currentItemChanged, this, [this] { sync(); });
        connect(tree_, &QTreeWidget::itemActivated, this, [this] { activate(); });
        connect(go_, &QPushButton::clicked, this, [this] { activate(); }); connect(cancel, &QPushButton::clicked, this, &QDialog::reject);
        connect(refresh_, &QPushButton::clicked, this, [this] { capture(); rebuild(true); });
        capture(); query_->setText(initial); rebuild(true); query_->setFocus();
    }
    ~PaletteDialog() override { if (context_) term_line_end(tab_, context_); }
    PaletteItem chosen;
    QPointer<PtyView> origin() const { return source_; }
    uint32_t generation() const { return generation_; }
    bool had_origin() const { return had_source_; }
    bool source_ready() const {
        if (!window_ || !palette_allowed() || window_->property("screenLocked").toBool()) return false;
        if (!had_source_) return !window_->active_view();
        if (!source_ || source_->is_closed() || window_->active_view() != source_) return false;
        const auto state = term_history_state(tab_); return state.available && state.generation == generation_;
    }
protected:
    bool eventFilter(QObject *object, QEvent *event) override {
        if ((object == query_ || object == tree_) && event->type() == QEvent::KeyPress) {
            auto *key = static_cast<QKeyEvent *>(event);
            if (key->key() == Qt::Key_Escape) { reject(); return true; }
            if (key->key() == Qt::Key_Return || key->key() == Qt::Key_Enter) { activate(); return true; }
            int delta = key->key() == Qt::Key_Down ? 1 : key->key() == Qt::Key_Up ? -1 : key->key() == Qt::Key_PageDown ? 8 : key->key() == Qt::Key_PageUp ? -8 : 0;
            if (delta && tree_->topLevelItemCount()) { const int at = qMax(0, tree_->indexOfTopLevelItem(tree_->currentItem())); tree_->setCurrentItem(tree_->topLevelItem(qBound(0, at + delta, tree_->topLevelItemCount() - 1))); return true; }
        }
        return QDialog::eventFilter(object, event);
    }
private:
    void capture() {
        if (context_) term_line_end(tab_, context_); context_ = 0; token_ = 0;
        source_ = window_ ? window_->active_view() : nullptr; had_source_ = bool(source_);
        tab_ = source_ ? source_->tab() : UINT32_MAX; generation_ = source_ ? term_history_state(tab_).generation : 0;
    }
    void switch_mode(PaletteMode mode) {
        query_->setText(prefix(mode) + drafts_[size_t(mode)]); query_->setFocus();
    }
    bool available(const PaletteItem &item) const {
        if (!window_ || !palette_allowed() || window_->property("screenLocked").toBool()) return false;
        if (item.kind == PaletteKind::Mode) return true;
        if (item.kind == PaletteKind::Live) return chrome::palette_live_ready(window_, item);
        if (item.kind == PaletteKind::Saved) return true;
        if (!source_ready()) return false;
        return item.kind != PaletteKind::Action || chrome::palette_action_ready(item);
    }
    void rebuild(bool force) {
        const Query parsed = parse(query_->text());
        if (parsed.mode != mode_) { drafts_[size_t(mode_)] = last_body_; mode_ = parsed.mode; force = true; }
        last_body_ = parsed.body; drafts_[size_t(mode_)] = parsed.body;
        { QSignalBlocker block(modes_); modes_->setCurrentIndex(int(mode_)); }
        const QString key = tree_->currentItem() ? tree_->currentItem()->data(0, Qt::UserRole + 1).toString() : QString();
        if (force) { model_ = {}; if (mode_ == PaletteMode::Actions || mode_ == PaletteMode::Sessions || source_ready()) model_ = chrome::palette_model(window_, source_, mode_); }
        tree_->clear(); token_ = 0;
        if (mode_ == PaletteMode::Lines) {
            if (!source_ready() || !source_) { status_->setText(line_error(7)); sync(); return; }
            if (!context_) { const auto state = term_line_begin(tab_, generation_); context_ = state.context; base_ = state.current; total_ = state.total; if (state.error) { status_->setText(line_error(state.error)); sync(); return; } }
            if (parsed.body.isEmpty()) { status_->setText(i18n::tr("输入行号或 ±N；基点 %1，保留范围 1–%2").arg(base_).arg(total_)); sync(); return; }
            const auto target = term_line_preview(tab_, generation_, context_, rs(parsed.body.toUtf8()));
            if (target.error) { status_->setText(line_error(target.error)); sync(); return; }
            token_ = target.token; auto *row = new QTreeWidgetItem(tree_); row->setText(0, i18n::tr("定位到行 %1").arg(target.line)); row->setText(1, q(target.text)); row->setToolTip(1, q(target.text)); row->setData(0, Qt::UserRole, -1); row->setData(0, Qt::UserRole + 1, QStringLiteral("line"));
            tree_->setCurrentItem(row); status_->setText(i18n::tr("确认后定位；持续输出和重排会追踪此来源，失效时请刷新。")); sync(); return;
        }
        const auto words = parsed.body.toLower().simplified().split(QLatin1Char(' '), Qt::SkipEmptyParts);
        int count = 0; QTreeWidgetItem *remembered = nullptr; QTreeWidgetItem *first = nullptr;
        for (int i = 0; i < model_.items.size(); ++i) {
            const auto &item = model_.items[i];
            const QString text = (item.title + QLatin1Char(' ') + item.detail + QLatin1Char(' ') + (item.kind == PaletteKind::Action || item.kind == PaletteKind::Mode ? QString::fromUtf8(item.tid) : QString())).toLower();
            if (!std::all_of(words.begin(), words.end(), [&](const QString &word) { return text.contains(word); })) continue;
            if (count == 512) break;
            auto *row = new QTreeWidgetItem(tree_); row->setText(0, item.title); row->setText(1, item.detail); row->setText(2, item.shortcut); row->setToolTip(0, item.title); row->setToolTip(1, item.detail);
            row->setData(0, Qt::UserRole, i); row->setData(0, Qt::UserRole + 1, item.key);
            if (!available(item)) row->setForeground(0, palette().brush(QPalette::Disabled, QPalette::Text));
            if (!first && available(item)) first = row; if (item.key == key) remembered = row; ++count;
        }
        tree_->setCurrentItem(remembered ? remembered : first ? first : tree_->topLevelItem(0));
        status_->setText(model_.error ? line_error(model_.error) : count ? i18n::tr("匹配 %1 项；最多显示 512 项").arg(count) : i18n::tr("没有匹配项，请修改输入或刷新"));
        if (model_.truncated) status_->setText(i18n::tr("候选已达上限；会话最多载入 4096 项，动作最多 1024 项。"));
        sync();
    }
    void sync() {
        auto *row = tree_->currentItem();
        const int index = row ? row->data(0, Qt::UserRole).toInt() : -1;
        go_->setEnabled(row && (mode_ == PaletteMode::Lines ? token_ != 0 && source_ready() : index >= 0 && index < model_.items.size() && available(model_.items[index])));
    }
    void activate() {
        if (committed_) return;
        auto *row = tree_->currentItem(); if (!row) return;
        if (mode_ == PaletteMode::Lines) {
            if (!source_ready() || !token_ || !term_line_commit(tab_, generation_, context_, token_)) { status_->setText(line_error(4)); token_ = 0; sync(); return; }
            chosen.kind = PaletteKind::Line; committed_ = true; accept(); return;
        }
        const int index = row->data(0, Qt::UserRole).toInt(); if (index < 0 || index >= model_.items.size()) return;
        const auto &item = model_.items[index];
        if (!available(item)) { status_->setText(item.unavailable.isEmpty() ? i18n::tr("当前状态不可用，请换一个项目或刷新") : item.unavailable); sync(); return; }
        if (item.kind == PaletteKind::Mode) { switch_mode(item.mode); return; }
        if (item.kind == PaletteKind::Saved && !palette_saved_exists(rs(item.saved.toUtf8()))) { status_->setText(line_error(4)); return; }
        chosen = item; committed_ = true; accept();
    }
    QPointer<MainWindow> window_; QPointer<PtyView> source_; bool had_source_ = false, committed_ = false;
    uint32_t tab_ = UINT32_MAX, generation_ = 0; uint64_t context_ = 0, token_ = 0, base_ = 0, total_ = 0;
    QComboBox *modes_; QLineEdit *query_; QTreeWidget *tree_; QLabel *status_; QPushButton *go_, *refresh_;
    PaletteMode mode_ = PaletteMode::Actions; chrome::PaletteModel model_; std::array<QString, 4> drafts_; QString last_body_;
};
}

void chrome::show_command_palette(MainWindow *window, const QString &initial) {
    QPointer<MainWindow> owner = window; if (!owner || !palette_allowed() || owner->property("screenLocked").toBool()) return;
    PaletteItem chosen; QPointer<PtyView> source; uint32_t generation = 0; bool had_source = false;
    {
        PaletteDialog dialog(window, initial); if (dialog.exec() != QDialog::Accepted) return;
        chosen = dialog.chosen; source = dialog.origin(); generation = dialog.generation(); had_source = dialog.had_origin();
    }
    if (!owner || !palette_allowed() || owner->property("screenLocked").toBool()) return;
    const auto same_source = [&] { return !had_source ? !owner->active_view() : source && !source->is_closed() && owner->active_view() == source && term_history_state(source->tab()).generation == generation; };
    if (chosen.kind == PaletteKind::Action) { if (same_source() && palette_action_ready(chosen)) chosen.action->trigger(); else owner->note(line_error(4)); }
    else if (chosen.kind == PaletteKind::Line) { if (source && !source->is_closed()) { source->sync_reading_mode(); source->update(); owner->refresh_chrome(); } }
    else if (chosen.kind == PaletteKind::Symbol) {
        if (same_source() && source && term_command_jump(source->tab(), chosen.command)) { source->sync_reading_mode(); source->update(); owner->refresh_chrome(); } else owner->note(line_error(4));
    } else if (chosen.kind == PaletteKind::Live) { if (palette_live_ready(owner, chosen)) owner->focus_session(chosen.view->tab()); else owner->note(line_error(4)); }
    else if (chosen.kind == PaletteKind::Saved) {
        const auto fresh = palette_model(owner, nullptr, PaletteMode::Sessions);
        for (const auto &item : fresh.items) if (item.kind == PaletteKind::Live && item.saved == chosen.saved && palette_live_ready(owner, item)) { owner->focus_session(item.view->tab()); return; }
        if (!palette_saved_exists(rs(chosen.saved.toUtf8()))) { owner->note(line_error(4)); return; }
        const auto saved = chosen.saved.toUtf8(); const uint32_t tab = sess_open_with_vault(owner, rust::String(saved.toStdString()));
        if (tab == UINT32_MAX || tab == kVaultLocked) { if (owner) owner->note(i18n::tr("已取消或无法打开所选会话")); return; }
        if (!owner || !palette_allowed() || owner->property("screenLocked").toBool()) { term_close_tab(tab); return; }
        owner->add_tab_with(tab); record_recent(chosen.saved); owner->refresh_chrome();
    }
}
