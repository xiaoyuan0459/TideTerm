// Integrated menu/caption; system move/resize retains OS snapping and monitor work areas.
#include "cpp/chrome/window_frame.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/quick_connect.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/icon_shapes.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QApplication>
#include <QHBoxLayout>
#include <QMenuBar>
#include <QMouseEvent>
#include <QPainter>
#include <QPointer>
#include <QShortcut>
#include <QToolButton>
#include <QWindow>
#include <cmath>

namespace chrome {
namespace {

void toggle_maximized(MainWindow *window) {
    if (window->isMaximized()) window->showNormal();
    else window->showMaximized();
}

class CaptionMenu : public QMenuBar {
public:
    explicit CaptionMenu(MainWindow *window) : QMenuBar(window), window_(window) {
        setNativeMenuBar(false);
    }

protected:
    void mousePressEvent(QMouseEvent *event) override {
        if (event->button() == Qt::LeftButton && !actionAt(event->position().toPoint()) &&
            window_->windowHandle() && window_->windowHandle()->startSystemMove()) {
            event->accept();
            return;
        }
        QMenuBar::mousePressEvent(event);
    }

    void mouseDoubleClickEvent(QMouseEvent *event) override {
        if (event->button() == Qt::LeftButton && !actionAt(event->position().toPoint())) {
            toggle_maximized(window_);
            event->accept();
            return;
        }
        QMenuBar::mouseDoubleClickEvent(event);
    }

private:
    MainWindow *window_;
};

enum class Control { Minimize, Maximize, Close };

class CaptionButton : public QToolButton {
public:
    CaptionButton(Control control, MainWindow *window, QWidget *parent)
        : QToolButton(parent), control_(control), window_(window) {
        setFixedSize(metrics::kWindowControlWidth, metrics::kMenuBarHeight - 1);
        setAutoRaise(true);
        setFocusPolicy(Qt::StrongFocus);
    }

protected:
    void paintEvent(QPaintEvent *event) override {
        QToolButton::paintEvent(event);
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);
        const qreal dpr = devicePixelRatioF();
        const qreal size = metrics::kWindowControlGlyph;
        painter.translate(std::round((width() - size) * dpr / 2) / dpr,
                          std::round((height() - size) * dpr / 2) / dpr);
        const QString name = control_ == Control::Minimize ? QStringLiteral("minimize")
            : control_ == Control::Close ? QStringLiteral("close")
            : window_->isMaximized() ? QStringLiteral("restore") : QStringLiteral("maximize");
        detail::paint_icon_shape(painter, name, size, palette().color(QPalette::ButtonText), dpr);
    }

private:
    Control control_;
    MainWindow *window_;
};

class FrameEvents : public QObject {
public:
    FrameEvents(MainWindow *window, CaptionButton *maximize)
        : QObject(window), window_(window), maximize_(maximize) {
        window_->installEventFilter(this);
        qApp->installEventFilter(this);
        update_control();
    }

protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        if (watched == window_) {
            if (event->type() == QEvent::WindowStateChange) {
                restore_cursor();
                update_control();
            }
        }
        if ((event->type() == QEvent::MouseButtonPress || event->type() == QEvent::MouseMove) &&
            !window_->isMaximized() && !window_->isFullScreen()) {
            auto *widget = qobject_cast<QWidget *>(watched);
            auto *mouse = static_cast<QMouseEvent *>(event);
            if (widget && widget->window() == window_) {
                const QPoint point = window_->mapFromGlobal(mouse->globalPosition().toPoint());
                Qt::Edges edges;
                if (point.x() < metrics::kWindowResizeBand) edges |= Qt::LeftEdge;
                if (point.x() >= window_->width() - metrics::kWindowResizeBand) edges |= Qt::RightEdge;
                if (point.y() < metrics::kWindowResizeBand) edges |= Qt::TopEdge;
                if (point.y() >= window_->height() - metrics::kWindowResizeBand) edges |= Qt::BottomEdge;
                update_cursor(widget, edges);
                if (edges && mouse->button() == Qt::LeftButton && window_->windowHandle() &&
                    window_->windowHandle()->startSystemResize(edges)) return true;
            }
        }
        if (event->type() == QEvent::Leave && watched == cursor_widget_) restore_cursor();
        return QObject::eventFilter(watched, event);
    }

private:
    void update_control() {
        const QString text = window_->isMaximized() ? i18n::tr("还原") : i18n::tr("最大化");
        maximize_->setToolTip(text);
        maximize_->setAccessibleName(text);
        maximize_->update();
    }

    void restore_cursor() {
        if (cursor_widget_) {
            if (had_cursor_) cursor_widget_->setCursor(previous_cursor_);
            else cursor_widget_->unsetCursor();
        }
        cursor_widget_.clear();
    }

    void update_cursor(QWidget *widget, Qt::Edges edges) {
        if (!edges || widget != cursor_widget_) restore_cursor();
        if (!edges) return;
        if (!cursor_widget_) {
            cursor_widget_ = widget;
            previous_cursor_ = widget->cursor();
            had_cursor_ = widget->testAttribute(Qt::WA_SetCursor);
        }
        const bool horizontal = edges.testFlag(Qt::LeftEdge) || edges.testFlag(Qt::RightEdge);
        const bool vertical = edges.testFlag(Qt::TopEdge) || edges.testFlag(Qt::BottomEdge);
        const bool descending = edges == (Qt::TopEdge | Qt::LeftEdge) || edges == (Qt::BottomEdge | Qt::RightEdge);
        widget->setCursor(horizontal && vertical ? (descending ? Qt::SizeFDiagCursor : Qt::SizeBDiagCursor) :
                          horizontal ? Qt::SizeHorCursor : Qt::SizeVerCursor);
    }
    QPointer<QWidget> cursor_widget_;
    QCursor previous_cursor_;
    bool had_cursor_ = false;
    MainWindow *window_;
    CaptionButton *maximize_;
};

} // namespace

void open_quick_connect(MainWindow *window) {
    const uint32_t id = run_quick_connect(window);
    if (id != UINT32_MAX) window->add_tab_with(id);
}

void focus_quick_connect(MainWindow *window) {
    auto *banner = window->page_banner(window->current_page());
    if (banner && banner->isVisible()) {
        banner->edit_address();
    } else {
        open_quick_connect(window);
    }
}

void install_window_frame(MainWindow *window) {
    window->setWindowFlag(Qt::FramelessWindowHint);
    auto *bar = new CaptionMenu(window);
    window->setMenuBar(bar);
    auto *controls = new QWidget(bar);
    controls->setObjectName(QStringLiteral("windowControls"));
    auto *layout = new QHBoxLayout(controls);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    auto *focus_quick = new QShortcut(QKeySequence(QStringLiteral("Ctrl+L")), window);
    QObject::connect(focus_quick, &QShortcut::activated, window, [window] {
        focus_quick_connect(window);
    });
    auto button = [&](Control type, const char *name, const QString &text, auto callback) {
        auto *control = new CaptionButton(type, window, controls);
        control->setObjectName(QLatin1String(name));
        control->setToolTip(text);
        control->setAccessibleName(text);
        layout->addWidget(control);
        QObject::connect(control, &QToolButton::clicked, window, callback);
        return control;
    };
    button(Control::Minimize, "windowMinimize", i18n::tr("最小化"), [window] { window->showMinimized(); });
    auto *maximize = button(Control::Maximize, "windowMaximize", i18n::tr("最大化"),
                            [window] { toggle_maximized(window); });
    button(Control::Close, "windowClose", i18n::tr("关闭窗口"), [window] { window->close(); });
    bar->setCornerWidget(controls, Qt::TopRightCorner);
    new FrameEvents(window, maximize);
}

void set_main_menus_visible(MainWindow *window, bool visible) {
    for (auto *action : window->menuBar()->actions()) action->setVisible(visible);
}

} // namespace chrome
