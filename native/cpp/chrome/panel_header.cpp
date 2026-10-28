// PanelHeader 实现（WT 对齐批次 1）。无 Q_OBJECT/moc：动作走 lambda + std::function。
#include "cpp/chrome/panel_header.h"

#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/panel_controls.h"
#include "cpp/chrome/panel_drag.h"
#include "cpp/chrome/ui_metrics.h"

#include <QDockWidget>
#include <QApplication>
#include <QKeyEvent>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QMouseEvent>
#include <QStyle>
#include <QToolButton>
#include <QVBoxLayout>

namespace chrome {

namespace {

/// 标题行右侧工具钮：按控件实际 DPI 绘制矢量图标。
QToolButton *make_tool_button(QWidget *parent, const char *name, const char *glyph) {
    auto *b = new QToolButton(parent);
    b->setObjectName(QLatin1String(name));
    chrome::set_button_icon(b, QString::fromUtf8(glyph), 14);
    b->setAutoRaise(true);
    b->setCursor(Qt::ArrowCursor);
    b->setFocusPolicy(Qt::NoFocus);
    b->setFixedSize(chrome::metrics::kPanelHeaderButtonSize,
                    chrome::metrics::kPanelHeaderButtonSize);
    return b;
}

} // namespace

PanelHeader::PanelHeader(const QString &title, QWidget *parent) : QWidget(parent) {
    setObjectName(QStringLiteral("panelHeader"));
    // 纯 QWidget 子类要走 QSS 背景/下边框，必须显式声明为 styled background；
    // 高度由布局（26 + 可选 22）决定，不被 dock 拉伸。
    setAttribute(Qt::WA_StyledBackground, true);
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
    setAutoFillBackground(false);
    auto *rows = new QVBoxLayout(this);
    rows->setContentsMargins(0, 0, 0, 0);
    rows->setSpacing(0);

    auto *row = new QWidget(this);
    row->setObjectName(QStringLiteral("panelHeaderRow"));
    row->setFixedHeight(chrome::metrics::kSectionHeaderHeight);
    auto *lay = new QHBoxLayout(row);
    lay->setContentsMargins(8, 0, 3, 0);
    lay->setSpacing(5);

    dot_ = new QLabel(row);
    dot_->setObjectName(QStringLiteral("panelDot"));
    dot_->setFixedSize(chrome::metrics::kPanelDotSize, chrome::metrics::kPanelDotSize);
    title_ = new QLabel(title, row);
    title_->setObjectName(QStringLiteral("panelTitle"));
    gear_ = make_tool_button(row, "panelGear", "settings");  // ⚙
    close_ = make_tool_button(row, "panelClose", "close"); // ✕
    gear_->setToolTip(i18n::tr("面板设置"));
    gear_->setAccessibleName(gear_->toolTip());
    close_->setToolTip(i18n::tr("隐藏面板"));
    close_->setAccessibleName(close_->toolTip());
    lay->addWidget(dot_);
    lay->addWidget(title_);
    lay->addStretch(1);
    lay->addWidget(gear_);
    lay->addWidget(close_);
    rows->addWidget(row);
}

void PanelHeader::set_title(const QString &title) {
    if (title_ != nullptr) {
        title_->setText(title);
    }
}

void PanelHeader::set_dot(const QString &kind) {
    if (dot_ == nullptr) {
        return;
    }
    // 逐面板异色（WT 截图 2026-09-25）：首字母大写拼进 objectName，QSS 里各有一条规则。
    const QString suffix = kind.isEmpty()
                               ? QString()
                               : kind.left(1).toUpper() + kind.mid(1);
    dot_->setObjectName(QStringLiteral("panelDot") + suffix);
    // objectName 变化不影响已应用的样式表，需重新 polish 才会重取规则。
    dot_->style()->unpolish(dot_);
    dot_->style()->polish(dot_);
}

QLineEdit *PanelHeader::add_filter(const QString &placeholder) {
    if (filter_ != nullptr) {
        return filter_;
    }
    filter_ = new QLineEdit(this);
    filter_->setObjectName(QStringLiteral("panelFilter"));
    filter_->setPlaceholderText(placeholder);
    filter_->setFixedHeight(chrome::metrics::kPanelFilterRowHeight);
    filter_->setClearButtonEnabled(true);
    QObject::connect(filter_, &QLineEdit::textChanged, this,
                     [this](const QString &text) {
                         if (on_filter) {
                             on_filter(text);
                         }
                     });
    // 筛选行在标题行之下（WT 面板头 = 标题 26 + 筛选 22）。
    auto *rows = qobject_cast<QVBoxLayout *>(layout());
    if (rows != nullptr) {
        rows->addWidget(filter_);
    }
    return filter_;
}

PanelHeader::~PanelHeader() { cancel_drag(); }

void PanelHeader::cancel_drag() {
    pressed_ = false;
    qApp->removeEventFilter(this);
    drag_.reset();
}

bool PanelHeader::eventFilter(QObject *object, QEvent *event) {
    if (pressed_) {
        if (event->type() == QEvent::KeyPress && static_cast<QKeyEvent *>(event)->key() == Qt::Key_Escape) {
            cancel_drag();
            return true;
        }
        // A native window move can change the receiver while this header still
        // owns the gesture. Remove the filter before a drop triggers hide/ungrab.
        if (event->type() == QEvent::MouseButtonRelease &&
            static_cast<QMouseEvent *>(event)->button() == Qt::LeftButton) {
            finish_drag(static_cast<QMouseEvent *>(event)->globalPosition().toPoint());
            return true;
        }
        if (event->type() == QEvent::MouseMove &&
            (static_cast<QMouseEvent *>(event)->buttons() & Qt::LeftButton)) {
            mouseMoveEvent(static_cast<QMouseEvent *>(event));
            return true;
        }
        auto *dock = qobject_cast<QDockWidget *>(parentWidget());
        if (event->type() == QEvent::ApplicationDeactivate ||
            (event->type() == QEvent::Hide && (object == this || object == dock)) ||
            (event->type() == QEvent::UngrabMouse && object == this)) cancel_drag();
    }
    return QWidget::eventFilter(object, event);
}

void PanelHeader::mousePressEvent(QMouseEvent *event) {
    auto *dock = qobject_cast<QDockWidget *>(parentWidget());
    if (event->button() == Qt::LeftButton && dock &&
        dock->features().testFlag(QDockWidget::DockWidgetMovable)) {
        cancel_drag();
        press_gp_ = event->globalPosition().toPoint();
        pressed_ = true;
        qApp->installEventFilter(this);
        event->accept();
        return;
    }
    QWidget::mousePressEvent(event);
}

void PanelHeader::mouseMoveEvent(QMouseEvent *event) {
    auto *dock = qobject_cast<QDockWidget *>(parentWidget());
    if (dock == nullptr || !pressed_ || !(event->buttons() & Qt::LeftButton)) {
        QWidget::mouseMoveEvent(event);
        return;
    }
    const QPoint gp = event->globalPosition().toPoint();
    if (!drag_) {
        if ((gp - press_gp_).manhattanLength() < QApplication::startDragDistance()) return;
        auto *window = qobject_cast<QMainWindow *>(dock->parentWidget());
        if (!window) return;
        drag_ = std::make_unique<PanelDrag>(window, dock, this);
        if (dock->isFloating()) drag_->set_float_grab(press_gp_);
    }
    drag_->update(gp);
    event->accept();
}

void PanelHeader::mouseReleaseEvent(QMouseEvent *event) {
    if (event->button() == Qt::LeftButton && pressed_) {
        finish_drag(event->globalPosition().toPoint());
        event->accept();
        return;
    }
    QWidget::mouseReleaseEvent(event);
}

void PanelHeader::finish_drag(const QPoint &global) {
    auto drag = std::move(drag_);
    cancel_drag();
    if (drag) drag->finish(global);
}

PanelHeader *install_panel_header(QDockWidget *dock, const QString &title,
                                  const QString &filter_placeholder) {
    if (dock == nullptr) {
        return nullptr;
    }
    auto *head = new PanelHeader(title, dock);
    dock->setTitleBarWidget(head);
    if (!filter_placeholder.isEmpty()) {
        head->add_filter(filter_placeholder);
    }
    QObject::connect(head->gear_button(), &QToolButton::clicked, dock, [dock, head] {
        QMenu menu(head);
        chrome::fill_panel_menu(&menu, dock);
        menu.exec(head->gear_button()->mapToGlobal(QPoint(0, head->gear_button()->height())));
    });
    QObject::connect(head->close_button(), &QToolButton::clicked, dock, [dock] {
        dock->close();
    });
    return head;
}

} // namespace chrome
