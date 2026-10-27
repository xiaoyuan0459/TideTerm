#include "cpp/chrome/transient_scrollbars.h"
#include "cpp/chrome/ui_metrics.h"

#include <QAbstractScrollArea>
#include <QEvent>
#include <QScrollBar>
#include <QSignalBlocker>
#include <QTimer>

namespace chrome {
namespace {

class TransientScrollbars : public QObject {
public:
    explicit TransientScrollbars(QAbstractScrollArea *view) : QObject(view), view_(view) {
        view_->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        view_->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        view_->installEventFilter(this);
        view_->viewport()->installEventFilter(this);
        view_->viewport()->setMouseTracking(true);
        expiry_.setSingleShot(true);
        connect(&expiry_, &QTimer::timeout, this, [this] {
            if (vertical_->isSliderDown() || horizontal_->isSliderDown()) {
                expiry_.start(metrics::kFileScrollHideDelay);
                return;
            }
            active_ = false;
            sync();
        });
        vertical_ = make_bar(Qt::Vertical, view_->verticalScrollBar());
        horizontal_ = make_bar(Qt::Horizontal, view_->horizontalScrollBar());
        sync();
    }

protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        switch (event->type()) {
        case QEvent::Enter:
            // A pointer entering a scrollable file pane should be able to grab
            // the thumb without first scrolling the contents.
            if (watched == vertical_ || watched == horizontal_) expiry_.stop();
            else QTimer::singleShot(0, this, [this] { reveal(); });
            break;
        case QEvent::Wheel:
            reveal();
            break;
        case QEvent::Leave:
        case QEvent::MouseButtonRelease:
            expiry_.start(metrics::kFileScrollHideDelay);
            break;
        case QEvent::Resize:
        case QEvent::Show:
            sync();
            break;
        case QEvent::Hide:
            if (watched == view_) {
                expiry_.stop();
                active_ = false;
                sync();
            }
            break;
        default:
            break;
        }
        return QObject::eventFilter(watched, event);
    }

private:
    QScrollBar *make_bar(Qt::Orientation orientation, QScrollBar *source) {
        auto *bar = new QScrollBar(orientation, view_->viewport());
        bar->setObjectName(QStringLiteral("fileScrollBar"));
        bar->setFocusPolicy(Qt::NoFocus);
        bar->setMouseTracking(true);
        bar->installEventFilter(this);
        bar->hide();
        connect(source, &QScrollBar::rangeChanged, this, [this] { sync(); });
        connect(source, &QScrollBar::valueChanged, this, [this] {
            // Automatic directory refreshes should not wake a dormant scrollbar.
            if (view_->hasFocus() || view_->viewport()->underMouse()) reveal();
            else sync();
        });
        connect(bar, &QScrollBar::valueChanged, source, &QScrollBar::setValue);
        connect(bar, &QScrollBar::sliderPressed, this, [this] { expiry_.stop(); });
        connect(bar, &QScrollBar::sliderReleased, this, [this] { reveal(); });
        return bar;
    }

    void reveal() {
        active_ = true;
        expiry_.start(metrics::kFileScrollHideDelay);
        sync();
    }

    void sync_bar(QScrollBar *bar, QScrollBar *source) {
        if (!bar) return;
        const QSignalBlocker blocker(bar);
        bar->setRange(source->minimum(), source->maximum());
        bar->setPageStep(source->pageStep());
        bar->setSingleStep(source->singleStep());
        bar->setValue(source->value());
        bar->setVisible(active_ && source->maximum() > source->minimum());
        bar->raise();
    }

    void sync() {
        sync_bar(vertical_, view_->verticalScrollBar());
        sync_bar(horizontal_, view_->horizontalScrollBar());
        if (!vertical_ || !horizontal_) return;
        const int thickness = metrics::kFileScrollBarWidth;
        const QRect area = view_->viewport()->rect().adjusted(2, 2, -2, -2);
        const int vcorner = horizontal_->isVisible() ? thickness + 2 : 0;
        const int hcorner = vertical_->isVisible() ? thickness + 2 : 0;
        vertical_->setGeometry(area.right() - thickness + 1, area.top(), thickness,
                               qMax(0, area.height() - vcorner));
        horizontal_->setGeometry(area.left(), area.bottom() - thickness + 1,
                                 qMax(0, area.width() - hcorner), thickness);
    }

    QAbstractScrollArea *view_;
    QScrollBar *vertical_ = nullptr;
    QScrollBar *horizontal_ = nullptr;
    QTimer expiry_;
    bool active_ = false;
};

} // namespace

void install_transient_scrollbars(QAbstractScrollArea *view) {
    new TransientScrollbars(view);
}

} // namespace chrome
