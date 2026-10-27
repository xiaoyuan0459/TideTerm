#include "cpp/terminal/terminal_history.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/hover_preview.h"
#include "cpp/i18n.h"
#include <QMouseEvent>
#include <QScrollBar>
#include <QSignalBlocker>
#include <QStyle>
#include <QStyleOptionSlider>

namespace chrome {
TerminalHistory::TerminalHistory(PtyView *view) : QObject(view), view_(view) {
    bar_ = new QScrollBar(Qt::Vertical, view);
    bar_->setObjectName(QStringLiteral("terminalHistoryBar"));
    bar_->setAccessibleName(i18n::tr("终端历史"));
    bar_->setFocusPolicy(Qt::StrongFocus);
    bar_->setMouseTracking(true);
    bar_->installEventFilter(this);
    view->installEventFilter(this);
    preview_ = new HoverPreview(bar_, view);
    bar_->hide();
    connect(bar_, &QScrollBar::valueChanged, this, [this](int value) {
        if (view_ && !view_->is_closed() && term_history_scroll_to(view_->tab(), uint32_t(value))) view_->update();
        if (bar_->isSliderDown()) preview_->refresh();
    });
}

void TerminalHistory::cancel() { preview_->cancel(); }

void TerminalHistory::sync() {
    if (!view_ || view_->is_closed()) { cancel(); bar_->hide(); return; }
    const auto state = term_history_state(view_->tab());
    const QSignalBlocker blocker(bar_);
    const QRect bounds(view_->width() - kWidth, metrics::kPanePadY, kWidth, view_->grid_h());
    if (bar_->geometry() != bounds) bar_->setGeometry(bounds);
    if (bar_->maximum() != int(state.maximum)) bar_->setRange(0, int(state.maximum));
    if (bar_->pageStep() != int(state.rows)) bar_->setPageStep(int(state.rows));
    if (!bar_->isSliderDown() && bar_->value() != int(state.position)) bar_->setValue(int(state.position));
    const bool visible = state.available && state.maximum > 0 && view_->width() > kWidth * 3;
    if (bar_->isHidden() == visible) bar_->setVisible(visible);
}

bool TerminalHistory::eventFilter(QObject *watched, QEvent *event) {
    if (watched == view_ && (event->type() == QEvent::Resize || event->type() == QEvent::Show)) sync();
    if (watched == view_ && (event->type() == QEvent::Hide || event->type() == QEvent::FocusOut ||
                            event->type() == QEvent::WindowDeactivate)) cancel();
    if (watched != bar_ || event->type() != QEvent::MouseMove || !view_ || view_->is_closed()) return false;
    const auto *mouse = static_cast<QMouseEvent *>(event);
    const int y = int(mouse->position().y());
    const int value = value_at(y);
    if (value < 0) { cancel(); return false; }
    preview_->request(uint64_t(value) + 1, mouse->globalPosition().toPoint(), [this, y] {
        const int current = value_at(y);
        const auto text = current >= 0 ? term_history_preview(view_->tab(), uint32_t(current)) : HistoryText{};
        if (!text.available) return i18n::tr("预览来源已失效，请关闭后重新打开");
        return i18n::tr("回滚预览 · 第 %1 行").arg(qulonglong(text.first_line)) + QLatin1Char('\n') +
            QString::fromUtf8(text.text.data(), int(text.text.size())) +
            (text.truncated ? QLatin1Char('\n') + i18n::tr("预览已限制为 8 KiB 或 32 行") : QString());
    });
    return false;
}

int TerminalHistory::value_at(int y) const {
    QStyleOptionSlider option;
    option.initFrom(bar_);
    option.orientation = Qt::Vertical;
    option.minimum = bar_->minimum(); option.maximum = bar_->maximum();
    option.sliderPosition = bar_->sliderPosition(); option.sliderValue = bar_->value();
    option.pageStep = bar_->pageStep(); option.singleStep = bar_->singleStep();
    const QRect groove = bar_->style()->subControlRect(QStyle::CC_ScrollBar, &option, QStyle::SC_ScrollBarGroove, bar_);
    const QRect thumb = bar_->style()->subControlRect(QStyle::CC_ScrollBar, &option, QStyle::SC_ScrollBarSlider, bar_);
    if (!groove.contains(QPoint(bar_->width() / 2, y))) return -1;
    return bar_->isSliderDown() || thumb.contains(QPoint(bar_->width() / 2, y)) ? bar_->sliderPosition() :
        QStyle::sliderValueFromPosition(bar_->minimum(), bar_->maximum(),
            qBound(0, y - groove.top() - thumb.height() / 2, qMax(1, groove.height() - thumb.height())),
            qMax(1, groove.height() - thumb.height()), false);
}
}
