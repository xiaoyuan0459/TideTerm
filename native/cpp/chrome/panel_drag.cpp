#include "cpp/chrome/panel_drag.h"
#include "cpp/chrome/panel_drop.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/ui_theme.h"
#include <QDockWidget>
#include <QGuiApplication>
#include <QMainWindow>
#include <QPainter>
#include <QScreen>

namespace chrome {
namespace {
class DragVisual : public QWidget {
public:
    DragVisual(QWidget *parent, const QString &title = {})
        : QWidget(parent, title.isEmpty() ? Qt::Widget : Qt::ToolTip | Qt::FramelessWindowHint), title_(title) {
        setAttribute(Qt::WA_TransparentForMouseEvents);
        setAttribute(Qt::WA_ShowWithoutActivating);
        setFocusPolicy(Qt::NoFocus);
        if (!title_.isEmpty()) {
            setAttribute(Qt::WA_TranslucentBackground);
            resize(qBound(140, fontMetrics().horizontalAdvance(title_) + 50, 240), 34);
        }
    }
protected:
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);
        const auto &colors = ui_colors();
        QColor fill = title_.isEmpty() ? colors.accent : colors.bg_elevated;
        fill.setAlpha(title_.isEmpty() ? 52 : 242);
        painter.setPen(QPen(colors.accent, 2));
        painter.setBrush(fill);
        painter.drawRoundedRect(QRectF(rect()).adjusted(1, 1, -1, -1), 5, 5);
        if (!title_.isEmpty()) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(colors.accent);
            painter.drawRoundedRect(QRect(12, 13, 8, 8), 2, 2);
            painter.setPen(colors.text);
            painter.drawText(rect().adjusted(28, 0, -10, 0), Qt::AlignVCenter,
                fontMetrics().elidedText(title_, Qt::ElideRight, width() - 38));
        }
    }
private:
    QString title_;
};
}

PanelDrag::PanelDrag(QMainWindow *window, QDockWidget *source, PanelHeader *header)
    : window_(window), source_(source), header_(header) {
    preview_ = new DragVisual(window);
    preview_->setObjectName(QStringLiteral("panelDropPreview"));
    card_ = new DragVisual(window, source->windowTitle());
    card_->setObjectName(QStringLiteral("panelDragCard"));
}
PanelDrag::~PanelDrag() {
    delete preview_.data();
    delete card_.data();
}
void PanelDrag::set_float_grab(const QPoint &global) {
    if (!source_) return;
    float_grab_ = global - source_->frameGeometry().topLeft();
    float_grab_set_ = true;
}
void PanelDrag::update(const QPoint &global) {
    if (!window_ || !source_ || !preview_ || !card_) return;
    // 浮动面板：OS 语义（跟手移动），只有指针回到主窗内才给停靠预览。
    // 之前的实现只挪标题卡、不动浮窗本身，浮窗看上去「拖不动」，只能靠松手时的落点跳一下。
    const bool floating = source_->isFloating();
    if (floating && !float_grab_set_) {
        // 首次收到应用级 MouseMove 时补记握持偏移：标题条里按下 → 事件过滤拿到移动时，
        // 用「面板当前外框左上角」反推，指针在面板内的位置就固定了。少了这一步，
        // 浮窗会相对指针漂移（松手落点也跟着错位）。
        set_float_grab(global);
    }
    QPoint target;
    if (floating) {
        target = float_grab_set_ ? global - float_grab_
                                 : global - QPoint(source_->width() / 2, 10);
        if (auto *screen = QGuiApplication::screenAt(global)) {
            const QRect available = screen->availableGeometry();
            target.setX(qBound(available.left(), target.x(),
                qMax(available.left(), available.right() - source_->width())));
            target.setY(qBound(available.top(), target.y(),
                qMax(available.top(), available.bottom() - source_->height())));
        }
        source_->move(target);
        source_->raise();
    }
    const auto drop = panel_drop_target(window_, source_, global);
    const bool in_window = window_->rect().contains(window_->mapFromGlobal(global));
    if (drop.preview.isEmpty() || !in_window) preview_->hide();
    else {
        preview_->setGeometry(drop.preview);
        preview_->show();
        preview_->raise();
    }
    // 浮窗跟手时，卡片贴着面板走；停靠拖动时，卡片跟随指针。
    QPoint position = floating ? target + QPoint(0, source_->height() + 8) : global + QPoint(16, 16);
    if (auto *screen = QGuiApplication::screenAt(global)) {
        const QRect available = screen->availableGeometry();
        if (position.x() + card_->width() > available.right()) position.setX(global.x() - card_->width() - 16);
        if (position.y() + card_->height() > available.bottom()) position.setY(global.y() - card_->height() - 16);
    }
    card_->move(position);
    card_->show();
}
void PanelDrag::finish(const QPoint &global) {
    if (!window_ || !source_) return;
    const auto drop = panel_drop_target(window_, source_, global);
    preview_->hide();
    card_->hide();
    apply_panel_drop(window_, source_, drop, global);
}
}
