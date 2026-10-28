#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/multiline_dialog.h"

void PtyView::mousePressEvent(QMouseEvent *e) {
    setFocus();
    const QPointF p = grid_pt(e->position());
    if (!closed_ && e->button() == Qt::LeftButton && e->modifiers() == Qt::NoModifier) {
        for (const auto &marker : term_fold_markers(tab_)) {
            if (!QRectF(marker.x, marker.y, marker.w, marker.h).contains(p)) continue;
            fold_press_ = true; selecting_ = false;
            term_command_fold(tab_, marker.id, !marker.folded);
            sync_reading_mode(); update(); e->accept(); return;
        }
    }
    const bool column = !closed_ && e->button() == Qt::LeftButton &&
        ((e->modifiers() & Qt::AltModifier) || term_column_selection(tab_));
    if (!column && !closed_ && !term_multi_selection(tab_) && e->button() == Qt::LeftButton && (e->modifiers() & Qt::ControlModifier) && !(e->modifiers() & Qt::ShiftModifier)) {
        const rust::String target = term_link_target(tab_, float(p.x()), float(p.y()));
        if (!target.empty()) {
            pressed_link_ = QString::fromUtf8(target.data(), int(target.size()));
            link_origin_ = e->position(); e->accept(); return;
        }
    }
    const uint8_t button = e->button() == Qt::LeftButton ? 0 :
        (e->button() == Qt::MiddleButton ? 1 : 2);
    if (!column && !closed_ && !term_multi_selection(tab_) && !(e->modifiers() & (Qt::ShiftModifier | Qt::ControlModifier)) &&
        term_mouse_send(tab_, 0, button, float(p.x()), float(p.y()))) {
        e->accept(); return;
    }
    if (e->button() == Qt::LeftButton) {
        selecting_ = !closed_ && extend_or_begin_selection(tab_, p, e->modifiers(), column);
        if (!closed_ && !selecting_) if (auto *win = dynamic_cast<MainWindow *>(window()))
            win->note(term_selection_locked(tab_) ? i18n::tr("选择已锁定，请先解锁或取消选择")
                                                 : i18n::tr("最多保留 32 个选区，请先删除或取消选择"));
        update();
    } else if (e->button() == Qt::RightButton) {
        if (!closed_ && !term_reading(tab_) && context_right_click_paste(tab_) && !(e->modifiers() & Qt::ControlModifier)) {
            chrome::paste_clipboard(this); update(); e->accept(); return;
        }
        // WT 对齐批次 4：19 项菜单在 chrome/term_menu.cpp（动作走菜单栏 dispatch 单点，
        // 缺后端的项置灰）。此处只负责弹。
        QMenu *menu = chrome::build_term_menu(this, term_command_at_point(tab_, float(p.x()), float(p.y())));
        menu->exec(e->globalPosition().toPoint());
        delete menu;
        update();
    }
    e->accept();
}

void PtyView::mouseMoveEvent(QMouseEvent *e) {
    if (fold_press_) { e->accept(); return; }
    const QPointF p = grid_pt(e->position());
    if (closed_) { e->accept(); return; }
    if (!pressed_link_.isEmpty()) {
        if ((e->position() - link_origin_).manhattanLength() < QApplication::startDragDistance()) {
            e->accept(); return;
        }
        pressed_link_.clear();
        const QPointF start = grid_pt(link_origin_);
        selecting_ = term_selection_begin_add(tab_, float(start.x()), float(start.y()), false);
    }
    if (e->modifiers() & Qt::ControlModifier) {
        const rust::String target = term_link_target(tab_, float(p.x()), float(p.y()));
        setCursor(target.empty() ? Qt::IBeamCursor : Qt::PointingHandCursor);
        setToolTip(QString::fromUtf8(target.data(), int(target.size())));
    } else { setCursor(Qt::IBeamCursor); setToolTip(QString()); }
    if (selecting_) { term_selection_update(tab_, float(p.x()), float(p.y())); update(); }
    else if (!(e->modifiers() & Qt::ControlModifier) && !term_multi_selection(tab_)) term_mouse_send(tab_, 1, e->buttons() & Qt::LeftButton ? 0 : 3,
                         float(p.x()), float(p.y()));
    e->accept();
}

void PtyView::mouseReleaseEvent(QMouseEvent *e) {
    if (fold_press_ && e->button() == Qt::LeftButton) { fold_press_ = false; e->accept(); return; }
    const QPointF p = grid_pt(e->position());
    if (closed_) { e->accept(); return; }
    if (!pressed_link_.isEmpty() && e->button() == Qt::LeftButton) {
        const QString target = std::exchange(pressed_link_, QString());
        if ((e->position() - link_origin_).manhattanLength() < QApplication::startDragDistance()) {
            if (!QDesktopServices::openUrl(QUrl(target))) setToolTip(i18n::tr("无法打开链接"));
        }
        e->accept(); return;
    }
    if (selecting_ && e->button() == Qt::LeftButton) {
        selecting_ = false;
        term_selection_end(tab_); // The injected Qt host publishes copy-on-select once.
        if (auto *win = dynamic_cast<MainWindow *>(window())) win->refresh_chrome();
        update();
    } else if (!(e->modifiers() & (Qt::ShiftModifier | Qt::ControlModifier)) && !term_multi_selection(tab_)) {
        const uint8_t button = e->button() == Qt::LeftButton ? 0 :
            (e->button() == Qt::MiddleButton ? 1 : 2);
        term_mouse_send(tab_, 2, button, float(p.x()), float(p.y()));
    }
    e->accept();
}


void PtyView::mouseDoubleClickEvent(QMouseEvent *event) {
    if (closed_ || event->button() != Qt::LeftButton) { event->accept(); return; }
    if (event->modifiers() & (Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier)) {
        mousePressEvent(event); return;
    }
    setFocus();
    const QPointF point = grid_pt(event->position());
    if (!(event->modifiers() & Qt::ShiftModifier) &&
        term_mouse_send(tab_, 0, 0, float(point.x()), float(point.y()))) {
        event->accept(); return;
    }
    selecting_ = term_selection_token(tab_, float(point.x()), float(point.y()));
    update();
    if (auto *win = dynamic_cast<MainWindow *>(window())) win->refresh_chrome();
    event->accept();
}
