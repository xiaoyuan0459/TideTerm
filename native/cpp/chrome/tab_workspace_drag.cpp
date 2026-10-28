#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/ui_theme.h"
#include <QApplication>
#include <QDrag>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QMimeData>
#include <QMouseEvent>
#include <QPainter>
#include <QTabBar>
#include <QTabWidget>
#include <QToolButton>

namespace { const char kMime[] = "application/x-tideterm-tab-group"; }

void TabWorkspace::start_drag(QTabWidget *group, QWidget *page, QWidget *handle) {
    auto *mime = new QMimeData;
    mime->setData(kMime, token_ + '\n' + group_name(group).toUtf8() + '\n' +
        QByteArray::number(page ? group->indexOf(page) : -1));
    auto *drag = new QDrag(this);
    drag->setMimeData(mime);
    // Only a small label is captured, never the terminal surface/history.
    QPixmap preview(220, 30); preview.fill(chrome::ui_colors().bg_elevated);
    QPainter painter(&preview); painter.setPen(chrome::ui_colors().term_fg);
    painter.drawText(preview.rect().adjusted(8, 0, -8, 0), Qt::AlignVCenter,
        page ? group->tabText(group->indexOf(page)) : group_name(group));
    painter.end(); drag->setPixmap(preview);
    pressed_handle_.clear(); pressed_group_.clear(); pressed_page_.clear();
    QMouseEvent release(QEvent::MouseButtonRelease, handle->mapFromGlobal(QCursor::pos()),
        QCursor::pos(), Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QApplication::sendEvent(handle, &release);
    drag->exec(Qt::MoveAction);
    drag->deleteLater();
}

bool TabWorkspace::decode_drag(const QMimeData *mime, QTabWidget **source, QWidget **page) const {
    if (!mime || !mime->hasFormat(kMime)) return false;
    const auto fields = mime->data(kMime).split('\n');
    if (fields.size() != 3 || fields[0] != token_) return false;
    bool ok = false;
    const int at = fields[2].toInt(&ok);
    if (!ok || at < -1) return false;
    for (auto *group : groups_) {
        if (group_name(group).toUtf8() != fields[1]) continue;
        if (at >= group->count()) return false;
        *source = group; *page = at < 0 ? nullptr : group->widget(at);
        return true;
    }
    return false;
}

void TabWorkspace::finish_drop(const QMimeData *mime, QTabWidget *target,
                              const QPoint &position, bool tab_bar) {
    QTabWidget *source = nullptr;
    QWidget *page = nullptr;
    if (!decode_drag(mime, &source, &page) || (source == target && !page)) return;
    int at = -1;
    if (tab_bar) at = target->tabBar()->tabAt(position);
    else {
        const QRect body = target->rect().adjusted(0, target->tabBar()->height(), 0, 0);
        const int x = position.x() - body.left(), y = position.y() - body.top();
        const int margin_x = qMin(100, body.width() / 5), margin_y = qMin(100, body.height() / 5);
        const bool horizontal = x < margin_x || x >= body.width() - margin_x;
        const bool vertical = y < margin_y || y >= body.height() - margin_y;
        if (horizontal || vertical) {
            if (source == target && source->count() <= 1) return;
            auto *created = split_group(target, horizontal ? Qt::Horizontal : Qt::Vertical,
                horizontal ? x < margin_x : y < margin_y);
            if (!created) return;
            target = created;
        }
    }
    if (page) move_page(page, target, at);
    else merge_group(source, target);
}

bool TabWorkspace::eventFilter(QObject *watched, QEvent *event) {
    QTabWidget *group = dynamic_cast<QTabWidget *>(watched);
    auto *bar = dynamic_cast<QTabBar *>(watched);
    auto *button = dynamic_cast<QToolButton *>(watched);
    if (bar) for (auto *candidate : groups_) if (candidate->tabBar() == bar) group = candidate;
    if (button) group = dynamic_cast<QTabWidget *>(button->property("dragTabGroup").value<QObject *>());
    if (!group || !groups_.contains(group)) return QWidget::eventFilter(watched, event);
    if (auto *mouse = dynamic_cast<QMouseEvent *>(event)) {
        if (event->type() == QEvent::MouseButtonPress && mouse->button() == Qt::LeftButton) {
            activate(group);
            if (bar || button) {
                pressed_handle_ = dynamic_cast<QWidget *>(watched);
                pressed_group_ = group;
                pressed_page_ = bar ? group->widget(bar->tabAt(mouse->position().toPoint())) : nullptr;
                pressed_position_ = mouse->position().toPoint();
            }
        } else if (event->type() == QEvent::MouseMove && (mouse->buttons() & Qt::LeftButton)
                   && pressed_handle_ == watched && pressed_group_ == group) {
            const QPoint pos = mouse->position().toPoint();
            const bool outside = button || !bar->rect().adjusted(-12, -12, 12, 12).contains(pos);
            if (outside && (pos - pressed_position_).manhattanLength() >= QApplication::startDragDistance()
                && (button || pressed_page_)) {
                start_drag(group, pressed_page_, pressed_handle_); return true;
            }
        } else if (event->type() == QEvent::MouseButtonRelease) {
            pressed_handle_.clear(); pressed_group_.clear(); pressed_page_.clear();
        }
    }
    if (event->type() == QEvent::DragEnter || event->type() == QEvent::DragMove) {
        auto *drag = static_cast<QDragMoveEvent *>(event);
        QTabWidget *source = nullptr; QWidget *page = nullptr;
        if (decode_drag(drag->mimeData(), &source, &page)) {
            drag->setDropAction(Qt::MoveAction); drag->accept(); return true;
        }
    } else if (event->type() == QEvent::Drop) {
        auto *drop = static_cast<QDropEvent *>(event);
        finish_drop(drop->mimeData(), group, drop->position().toPoint(), bar != nullptr);
        drop->setDropAction(Qt::MoveAction); drop->accept(); return true;
    }
    return QWidget::eventFilter(watched, event);
}
