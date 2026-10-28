// MainWindow 窗格/分割树域（Phase C ②）：分割、关格、遍历与折叠提升。
// 从 main_window.cpp 拆出（2026-09-28，该文件过 500 行 C++ 硬限）；会话状态仍在 Rust 注册表。
#include "cpp/main_window.h"
#include "cpp/chrome/text_marks.h"

#include "cpp/chrome/panel_splits.h"
#include "cpp/terminal/terminal_view.h"

#include <QSplitter>
#include "cpp/chrome/docks.h"
#include "cpp/chrome/shell_panel.h"

std::vector<PtyView *> MainWindow::views(QWidget *root) {
    std::vector<PtyView *> out;
    collect(root, &out);
    return out;
}

void MainWindow::split_active(Qt::Orientation o) {
    PtyView *v = active_view();
    if (!v) {
        return;
    }
    auto *ps = dynamic_cast<QSplitter *>(v->parentWidget());
    if (!ps) {
        return; // 页根必是 splitter（add_tab 保证）
    }
    // 放大态下分割语义混乱——先还原尺寸再分。
    if (zoom_splitter_ == ps && zoomed_ != nullptr) {
        if (ps->count() == int(zoom_sizes_.size())) {
            ps->setSizes(QList<int>(zoom_sizes_.begin(), zoom_sizes_.end()));
        }
        zoom_splitter_ = nullptr;
        zoomed_ = nullptr;
        zoom_sizes_.clear();
    }
    const int i = ps->indexOf(v);
    const QSize previous_size = v->size();
    const QList<int> parent_sizes = ps->sizes();
    const uint32_t id = term_split_tab(v->tab());
    if (id == UINT32_MAX) {
        note(i18n::tr("串口不能重复打开"));
        return;
    }
    auto *ns = chrome::panel_splitter(o);
    chrome::install_panel_split(ns);
    // Replace before reparenting: removing the only child first collapses the
    // parent and briefly resizes ConPTY to one row, erasing its live prompt.
    ps->replaceWidget(i, ns);
    ns->addWidget(v);
    term_set_render_opts(id, sess_active_config(id).options.cursor_style, true);
    auto *created = new PtyView(id, true);
    ns->addWidget(created);
    ns->show();
    v->show();
    created->show();
    const int span = o == Qt::Horizontal ? previous_size.width() : previous_size.height();
    ps->setSizes(parent_sizes);
    ns->setSizes({qMax(1, span / 2), qMax(1, span / 2)});
    created->setFocus();
    refresh_chrome();
}

void MainWindow::close_pane(PtyView *v) {
    if (docks_->shells && docks_->shells->active_view() == v) {
        docks_->shells->close_shell(docks_->shells->currentIndex()); return;
    }
    auto *ps = dynamic_cast<QSplitter *>(v->parentWidget());
    if (!ps) {
        return;
    }
    // 放大态还原（否则剩余窗格尺寸 0 永不可见）。
    if (zoom_splitter_ != nullptr && (zoomed_ == v || zoom_splitter_ == ps)) {
        if (zoom_splitter_->count() == int(zoom_sizes_.size())) {
            zoom_splitter_->setSizes(QList<int>(zoom_sizes_.begin(), zoom_sizes_.end()));
        }
        zoom_splitter_ = nullptr;
        zoomed_ = nullptr;
        zoom_sizes_.clear();
    }
    // 找到所属页；页内窗格总数 ≤1 ⇒ 关整页。
    int page = -1;
    int pane_total = 0;
    for (int i = 0; i < int(page_roots_.size()); ++i) {
        const int n = int(views(page_roots_[size_t(i)]).size());
        if (is_ancestor(page_roots_[size_t(i)], v)) {
            page = i;
            pane_total = n;
        }
    }
    if (page >= 0 && pane_total <= 1) {
        close_tab(page);
        return;
    }
    if (!chrome::prepare_mark_close(this, {v})) return;
    if (page >= 0) {
        for (PtyView *other : views(page_roots_[size_t(page)])) {
            if (other != v) {
                active_panes_[page_roots_[size_t(page)]] = other;
                if (v->hasFocus()) other->setFocus();
                break;
            }
        }
    }
    // QSplitter 无 removeWidget —— setParent(nullptr) 摘出后再销毁。
    v->setParent(nullptr);
    v->mark_closed(); // 先封桥：焦点迁移/排队事件不得再发 term_*（同 close_tab）
    term_close_tab(v->tab());
    v->deleteLater();
    collapse_if_single(ps);
    if (page >= 0) {
        auto *next = first_view(page_roots_[size_t(page)]);
        active_panes_[page_roots_[size_t(page)]] = next;
        if (next) next->setFocus();
    }
    refresh_chrome();
}

void MainWindow::collect(QWidget *w, std::vector<PtyView *> *out) {
    if (auto *splitter = dynamic_cast<QSplitter *>(w)) {
        for (int i = 0; i < splitter->count(); ++i) collect(splitter->widget(i), out);
        return; // Visual pane order is independent of QObject creation order.
    }
    if (auto *p = dynamic_cast<PtyView *>(w)) {
        out->push_back(p);
    }
    for (QObject *c : w->children()) {
        if (auto *cw = dynamic_cast<QWidget *>(c)) {
            collect(cw, out);
        }
    }
}

bool MainWindow::is_ancestor(QWidget *root, QWidget *w) {
    for (QWidget *a = w; a; a = a->parentWidget()) {
        if (a == root) {
            return true;
        }
    }
    return false;
}

PtyView *MainWindow::first_view(QWidget *root) {
    auto vs = views(root);
    return vs.empty() ? nullptr : vs[0];
}

/// splitter 只剩一个子时，把子提升到父位（防嵌套碎片化）。
void MainWindow::collapse_if_single(QSplitter *ps) {
    if (ps->count() != 1) {
        return;
    }
    QWidget *only = ps->widget(0);
    auto *gps = dynamic_cast<QSplitter *>(ps->parentWidget());
    if (gps) {
        const int gi = gps->indexOf(ps);
        const QList<int> sizes = gps->sizes();
        // QSplitter 无 removeWidget —— hide + setParent(nullptr) 摘出防顶层闪烁，
        // 再 insertWidget 放回父位（gi 是 ps 自身的位置，不受 only 摘出影响）。
        only->hide();
        only->setParent(nullptr);
        gps->replaceWidget(gi, only);
        only->show();
        // Detaching the last child can collapse its slot before replacement.
        // Preserve the user's allocation for the promoted pane and its siblings.
        gps->setSizes(sizes);
        ps->deleteLater();
    }
    // ps 是页内根 splitter（父 = 页根 QWidget）时保留单子 splitter —— 无碍。
}
