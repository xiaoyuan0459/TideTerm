#include "cpp/terminal/terminal_completion.h"
#include "cpp/chrome/completion_labels.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/i18n.h"
#include <QAbstractItemModel>
#include <QHeaderView>
#include <QInputMethodEvent>
#include <QScreen>
#include <QStyledItemDelegate>
#include <QStyleOptionViewItem>
#include <QTreeWidget>
#include <algorithm>

namespace {
class SourceBadge final : public QStyledItemDelegate {
public:
    using QStyledItemDelegate::QStyledItemDelegate;
    void paint(QPainter *painter, const QStyleOptionViewItem &option, const QModelIndex &index) const override {
        if (index.data().toString().isEmpty()) return;
        const auto &colors = chrome::ui_colors();
        painter->save();
        painter->fillRect(option.rect, colors.accent);
        const int background = qGray(colors.accent.rgb());
        painter->setPen(std::abs(background - qGray(colors.text.rgb())) >= std::abs(background - qGray(colors.term_bg.rgb()))
            ? colors.text : colors.term_bg);
        QFont font = option.font; font.setBold(true); painter->setFont(font);
        painter->drawText(option.rect, Qt::AlignCenter, index.data().toString());
        painter->restore();
    }
};
QString source_message(const TerminalCompletion &value) {
    if (value.batch.shell.state == 6 || value.batch.shell.state == 7) return i18n::tr("来源快照已失效，请从 Shell 环境重新采集。");
    const auto &shell = value.batch.shell;
    auto missing = [&](uint8_t kind) { return std::none_of(value.batch.items.begin(), value.batch.items.end(), [kind](const auto &item) { return item.kind == kind; }); };
    if (shell.name_catalog && shell.truncated) return i18n::tr("仅保留部分适用名字；可填写更具体的采集前缀后刷新。");
    if (shell.name_catalog && missing(12)) return i18n::tr("名字目录没有匹配项，可从 Shell 环境重新采集。");
    if (!shell.kube.config.empty() && shell.truncated) return i18n::tr("仅保留部分资源；不代表完整 namespace 列表。");
    if (!shell.kube.config.empty() && missing(11)) return i18n::tr("资源快照没有匹配项。");
    switch (value.environment_state) {
    case 1: return i18n::tr("正在查找会话建议…");
    case 2:
        if (value.environment_truncated) return i18n::tr("仅显示部分会话建议，请缩小查询或目录范围。");
        if (value.environment_skipped) return i18n::tr("已省略需要引用或无法直接插入的名称。");
        return QString();
    case 3: return i18n::tr("此会话暂不支持路径建议；不会改用本机目录。");
    case 4: return i18n::tr("等待会话报告工作目录。");
    case 5: return i18n::tr("仅支持简单草稿末尾的显式路径；引用参数请先选择对应 Shell 语法。");
    case 6: return i18n::tr("无法读取路径，请检查目录和访问权限后重试。");
    case 7: return i18n::tr("路径扫描超时，请重试。");
    case 8: return i18n::tr("路径建议暂时繁忙，请关闭其他建议后重试。");
    case 9: return i18n::tr("路径建议来源已改变，请重新打开建议。");
    case 10: return i18n::tr("远端 SFTP 路径服务不可用，请检查服务器后重试；不会改用本机目录。");
    case 11: return i18n::tr("此 SSH 会话已禁用 SFTP，请先在会话设置中启用。");
    case 12: return i18n::tr("请先配置当前会话的程序目录。");
    case 13: return i18n::tr("程序目录需为该会话的绝对路径，最多 8 个、合计 16 KiB；不支持 UNC、设备路径或控制字符。");
    case 14: return i18n::tr("程序目录属于另一会话或连接版本，请重新配置。");
    default: return QString();
    }
}
}

TerminalSuggestions::TerminalSuggestions(PtyView *view) : QObject(view), view_(view), popup_(new QTreeWidget(view)) {
    popup_->setObjectName(QStringLiteral("terminalSuggestions"));
    popup_->setWindowFlags(Qt::ToolTip | Qt::WindowDoesNotAcceptFocus);
    popup_->setFocusPolicy(Qt::NoFocus);
    popup_->setRootIsDecorated(false); popup_->setHeaderHidden(true);
    popup_->setIndentation(0);
    popup_->setColumnCount(2); popup_->setUniformRowHeights(true);
    popup_->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    popup_->header()->setStretchLastSection(false);
    popup_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    popup_->header()->setSectionResizeMode(1, QHeaderView::Fixed);
    popup_->setItemDelegateForColumn(1, new SourceBadge(popup_));
    popup_->setTextElideMode(Qt::ElideRight);
    popup_->setAccessibleName(i18n::tr("终端命令建议（仅填入输入）"));
    popup_->hide(); view_->installEventFilter(this);
    delay_.setSingleShot(true); delay_.setInterval(40);
    connect(&delay_, &QTimer::timeout, this, [this] { refresh(); });
    connect(popup_, &QTreeWidget::itemClicked, this, [this](QTreeWidgetItem *, int) { accept(); });
}

void TerminalSuggestions::cancel() { editing_ = false; explicit_list_ = false; delay_.stop(); popup_->hide(); terminal_completion_cancel(view_->tab()); }

void TerminalSuggestions::sync(bool repaint) {
    if (view_->is_closed() || !view_->hasFocus() || !view_->isVisible() || term_reading(view_->tab())) {
        cancel(); return;
    }
    if (!editing_ && !popup_->isVisible()) return;
    const bool valid = completion_valid(snapshot_.batch.context);
    if (popup_->isVisible() && !valid) popup_->hide();
    if (popup_->isVisible() && (repaint || view_geometry_ != QRect(view_->mapToGlobal(QPoint()), view_->size()))) {
        apply_style(); place_popup();
    }
    if (editing_ && repaint && view_->preedit().isEmpty()) {
        const auto state = term_history_state(view_->tab());
        if (!state.available) { cancel(); return; }
        // Repaint includes cursor blinking. Reuse the existing output revision
        // so idle paints never reset the list or rerun the candidate query.
        if (!valid || state.revision != snapshot_.output_revision || state.generation != snapshot_.batch.context.generation)
            delay_.start();
    }
}

bool TerminalSuggestions::eventFilter(QObject *, QEvent *event) {
    switch (event->type()) {
    case QEvent::Hide: case QEvent::FocusOut: case QEvent::WindowDeactivate:
    case QEvent::Wheel: case QEvent::MouseButtonPress:
        cancel(); break;
    case QEvent::Resize:
        // Resizing retires the caret list; a later edit or Ctrl+Space reopens it
        // from the current draft rather than keeping stale geometry alive.
        popup_->hide(); if (editing_) delay_.start(); break;
    case QEvent::InputMethod: {
        const auto *ime = static_cast<QInputMethodEvent *>(event);
        popup_->hide();
        if (!ime->commitString().isEmpty()) {
            explicit_list_ = false; editing_ = true; delay_.start();
        }
        break;
    }
    case QEvent::ShortcutOverride: {
        const auto *key = static_cast<QKeyEvent *>(event);
        if ((key->key() == Qt::Key_Space && key->modifiers() == Qt::ControlModifier) ||
            (popup_->isVisible() && !snapshot_.batch.items.empty() && (key->key() == Qt::Key_Tab || key->key() == Qt::Key_Up || key->key() == Qt::Key_Down))) {
            event->accept(); return true;
        }
        break;
    }
    case QEvent::KeyRelease:
        if (static_cast<QKeyEvent *>(event)->key() == accepting_key_) accepting_key_ = 0;
        break;
    case QEvent::KeyPress: {
        const auto *key = static_cast<QKeyEvent *>(event);
        if (accepting_key_ && key->key() == accepting_key_ && key->isAutoRepeat()) return true;
        if (!view_->preedit().isEmpty() || view_->is_closed()) break;
        if (key->key() == Qt::Key_Space && key->modifiers() == Qt::ControlModifier) {
            editing_ = true; explicit_list_ = true; refresh(); return true;
        }
        if (popup_->isVisible() && !snapshot_.batch.items.empty()) {
            if (key->key() == Qt::Key_Escape) { cancel(); return true; }
            if (key->key() == Qt::Key_Up || key->key() == Qt::Key_Down) {
                const int row = popup_->indexOfTopLevelItem(popup_->currentItem());
                const int step = key->key() == Qt::Key_Down ? 1 : -1;
                popup_->setCurrentItem(popup_->topLevelItem(std::clamp(row + step, 0, int(snapshot_.batch.items.size()) - 1)));
                popup_->scrollToItem(popup_->currentItem());
                return true;
            }
            if (key->key() == Qt::Key_Tab || key->key() == Qt::Key_Return || key->key() == Qt::Key_Enter) {
                accepting_key_ = key->key(); if (!key->isAutoRepeat()) accept(); return true;
            }
        }
        if (key->key() == Qt::Key_Control || key->key() == Qt::Key_Shift || key->key() == Qt::Key_Alt) break;
        const bool text = !key->text().isEmpty() && key->text().front().isPrint();
        explicit_list_ = false;
        editing_ = !(key->modifiers() & (Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier)) &&
            (text || key->key() == Qt::Key_Backspace);
        popup_->hide(); if (editing_) delay_.start();
        break;
    }
    default: break;
    }
    return false;
}

void TerminalSuggestions::apply_style() {
    const auto &colors = chrome::ui_colors();
    const auto family = term_font_family();
    QFont font(QString::fromUtf8(family.data(), int(family.size())));
    font.setPixelSize(int(term_font_px()));
    if (popup_->font() != font) popup_->setFont(font);
    // The application stylesheet supplies its own font size; override it here
    // as well as setFont so the list follows terminal text rather than chrome.
    const QString face = font.family().replace(QLatin1Char('\\'), QStringLiteral("\\\\")).replace(QLatin1Char('"'), QStringLiteral("\\\""));
    const QString style = QStringLiteral("QTreeWidget { background:%1; color:%2; border:1px solid %3; font-size:%5px; font-family:\"%6\"; } QTreeWidget::item { padding:2px 4px; } QTreeWidget::item:selected { background:%4; color:%2; }")
        .arg(colors.bg_elevated.name(), colors.text.name(), colors.border1.name(), colors.accent.name()).arg(term_font_px()).arg(face);
    if (style_ != style) { style_ = style; popup_->setStyleSheet(style); }
    popup_->ensurePolished();
    popup_->setColumnWidth(1, popup_->fontMetrics().horizontalAdvance(QLatin1Char('c')) + 12);
}

void TerminalSuggestions::refresh() {
    if (!editing_ || !view_->preedit().isEmpty() || !view_->hasFocus() || view_->is_closed()) { cancel(); return; }
    auto next = terminal_completion_query(view_->tab());
    if (next.query.empty() && !explicit_list_) { cancel(); return; }
    const auto info = source_message(next);
    if (next.environment_state == 1) delay_.start(100);
    if (!next.batch.context.ready || (next.batch.items.empty() && info.isEmpty())) { snapshot_ = std::move(next); popup_->hide(); return; }
    bool stable = next.batch.context.shell_kind==snapshot_.batch.context.shell_kind && next.batch.context.system==snapshot_.batch.context.system
        && next.query == snapshot_.query && next.batch.items.size() == snapshot_.batch.items.size()
        && popup_->topLevelItemCount() == int(next.batch.items.size()) + int(!info.isEmpty()) && info == info_;
    for (size_t at = 0; stable && at < next.batch.items.size(); ++at) {
        const auto &old = snapshot_.batch.items[at]; const auto &item = next.batch.items[at];
        stable = old.command == item.command && old.kind == item.kind && old.user_id == item.user_id && old.reference == item.reference;
    }
    const QString selected = popup_->currentItem() ? popup_->currentItem()->text(0) : QString();
    snapshot_ = std::move(next);
    info_ = info;
    if (!stable) {
        popup_->clear(); QTreeWidgetItem *selection = nullptr;
        for (const auto &item : snapshot_.batch.items) {
            const QString value = QString::fromUtf8(item.command.data(), int(item.command.size()));
            const QString kind = chrome::completion_origin_label(item.kind);
            auto *row = new QTreeWidgetItem(popup_, {value, chrome::completion_origin_badge(item.kind)});
            row->setData(1, Qt::AccessibleTextRole, kind);
            const QString context=i18n::tr("当前环境：%1 / %2；来源：%3").arg(chrome::completion_shell_label(snapshot_.batch.context.shell_kind),chrome::completion_system_label(snapshot_.batch.context.system),kind);
            const QString detail=item.kind==13 ? QString::fromUtf8(item.detail.data(),int(item.detail.size())) : QString();
            row->setToolTip(0,QStringLiteral("<qt>")+value.toHtmlEscaped()+QStringLiteral("<br>")+context.toHtmlEscaped()+QStringLiteral("<br>")+detail.toHtmlEscaped().replace(QLatin1Char('\n'),QStringLiteral("<br>"))+QStringLiteral("<br>")+i18n::tr("仅填入当前终端，不执行；再次按 Enter 才发送。").toHtmlEscaped()+QStringLiteral("</qt>"));
            row->setToolTip(1, context);
            if (value == selected) selection = row;
        }
        if (!info_.isEmpty()) { auto *row = new QTreeWidgetItem(popup_, {info_, QString()}); row->setFlags(Qt::NoItemFlags); row->setToolTip(0, info_); }
        if (!snapshot_.batch.items.empty()) popup_->setCurrentItem(selection ? selection : popup_->topLevelItem(0));
    }
    apply_style(); place_popup();
}

void TerminalSuggestions::place_popup() {
    view_geometry_ = QRect(view_->mapToGlobal(QPoint()), view_->size());
    const QRect bounds = view_geometry_.intersected(view_->screen()->availableGeometry());
    const QRectF caret = view_->ime_query(Qt::ImCursorRectangle).toRectF();
    const QPoint below = view_->mapToGlobal(QPoint(int(caret.x()), int(caret.bottom())));
    const QPoint top = view_->mapToGlobal(caret.topLeft().toPoint());
    if (!bounds.contains(QRect(top, caret.size().toSize()).center())) { popup_->hide(); return; }
    QStyleOptionViewItem option; option.initFrom(popup_); option.widget = popup_;
    option.font = popup_->font(); option.fontMetrics = QFontMetrics(option.font);
    int row_height = popup_->fontMetrics().height() + 4;
    int text_width = 0;
    // Ask Qt for the same styled cell size it paints, including platform margins.
    for (int row = 0; row < popup_->topLevelItemCount(); ++row) {
        const QSize natural = popup_->itemDelegate()->sizeHint(option, popup_->model()->index(row, 0));
        text_width = std::max(text_width, natural.width());
        row_height = std::max(row_height, natural.height());
        if (text_width >= bounds.width()) break;
    }
    const int frame = 2 * popup_->frameWidth();
    const int down = std::max(0, bounds.bottom() - below.y() + 1);
    const int up = std::max(0, below.y() - int(caret.height()) - bounds.top());
    const int rows = std::min({8, popup_->topLevelItemCount(), (std::max(down, up) - frame) / row_height});
    const int height = row_height * rows + frame;
    const int scroll = popup_->topLevelItemCount() > rows ? popup_->style()->pixelMetric(QStyle::PM_ScrollBarExtent) : 0;
    const int width = std::min(bounds.width(), text_width + popup_->columnWidth(1) + scroll + frame + 2);
    if (rows < 1 || width < popup_->columnWidth(1) + 32) { popup_->hide(); return; }
    const int x = std::clamp(below.x(), bounds.left(), bounds.right() - width + 1);
    const int y = down >= height ? below.y() : below.y() - int(caret.height()) - height;
    popup_->setGeometry(x, std::max(bounds.top(), y), width, height); popup_->show();
}

void TerminalSuggestions::accept() {
    const int at = popup_->indexOfTopLevelItem(popup_->currentItem());
    const bool valid = !view_->is_closed() && view_->hasFocus() && at >= 0 &&
        terminal_completion_accept(snapshot_, uint32_t(at));
    cancel();
    if (!valid) if (auto *window = dynamic_cast<MainWindow *>(view_->window()))
        window->note(i18n::tr("建议已失效或当前输入不可写，请重新打开建议。"));
}
