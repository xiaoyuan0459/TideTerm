// MainWindow：标签/分割/chrome 编排；会话状态全在 Rust 注册表。
#include "cpp/main_window.h"
#include "cpp/chrome/text_marks.h"

#include "cpp/chrome/ai_panel.h"
#include "cpp/chrome/app_font.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/favorite_toolbar.h"
#include "cpp/chrome/menu_bar.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/prompts.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/status_bar.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/chrome/tab_quick_bar.h"
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/panel_controls.h"
#include "cpp/chrome/panel_splits.h"
#include "cpp/chrome/window_frame.h"
#include "cpp/i18n.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"

#include <QApplication>
#include <QCursor>
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QMenu>
#include <QMenuBar>
#include <QScreen>
#include <QSplitter>
#include <QStatusBar>
#include <QTabWidget>
#include <QTimer>
#include <QVBoxLayout>
#include <cstdio>
#include "cpp/chrome/shell_panel.h"
#include "cpp/chrome/screen_lock.h"
#include <QPointer>
#include "cpp/chrome/multiline_dialog.h"

MainWindow::MainWindow(bool open_first_tab) {
    chrome::init_app_font();
    chrome::install_window_frame(this);
    // HiDPI：把设备像素比下发引擎，使终端字形按**物理分辨率**光栅化。否则引擎按逻辑
    // 像素渲染、QImage 再被 dpr 放大 → 文字发虚成马赛克。窗口尚未 show，先取主屏
    // dpr；PtyView::showEvent 落到具体屏幕后会再校正一次（多屏不同 dpr）。
    if (const QScreen *scr = QGuiApplication::primaryScreen()) {
        term_set_scale(static_cast<float>(scr->devicePixelRatio()));
    }
    resize(1200, 800);
    setProperty("highlightCaretLine", true);

    workspace_ = new TabWorkspace(this);
    tabs_ = workspace_->first_group();
    workspace_->on_active = [this](QTabWidget *group) { activate_group(group); };
    workspace_->on_changed = [this] { sync_send_panel(); refresh_chrome(); };
    workspace_->on_reorder = [this](QTabWidget *group) { sync_group_order(group); };
    workspace_->on_close = [this](QWidget *root) { chrome::request_close_tab(this, page_index(root)); };
    setCentralWidget(workspace_);

    docks_ = chrome::build_docks(this);
    build_send_panel();
    chrome::install_panel_shortcuts(this);
    // 主题 QSS 首次应用（⑦；ui_prefs 兼容读取，含 legacy config.json 迁移）。
    apply_theme();
    chrome::build_menu(this);
    // 常用动作工具栏不再挂窗口级 TopToolBarArea：2026-10-01 起随标签页装配在连接栏上排
    // （见 add_tab_with），分屏时与连接栏一起留在页头。
    chrome::build_status_bar(this); // ⑧ 右侧永久字段（主题/后端/fps/时钟）
    restore_layout();               // 对齐批次 A：标签条度量 + 上次 dock/窗口状态
    statusBar()->showMessage(i18n::tr("就绪"));

    connect(qApp, &QApplication::focusChanged, this, [this](QWidget *, QWidget *now) {
        if (!now) return;
        if (docks_->shells && docks_->shells->owns_focus()) { refresh_chrome(); return; }
        auto *pane = dynamic_cast<PtyView *>(now);
        for (QWidget *root : page_roots_) {
            if (is_ancestor(root, now)) {
                if (pane) active_panes_[root] = pane;
                if (auto *group = workspace_->owner(root)) tabs_ = group;
                sync_send_panel();
                refresh_chrome();
                break;
            }
        }
    });

    pump_ = new QTimer(this);
    QElapsedTimer pump_clock;
    pump_clock.start();
    connect(pump_, &QTimer::timeout, this, [this, pump_clock, last_chrome = qint64(0),
            last_status = qint64(0), last_redraw = qint64(-1000), fast_until = qint64(0), dense_redraws = 0,
            diagnostic_pumps = 0]() mutable {
        const qint64 now = pump_clock.elapsed();
        const bool refresh = now - last_chrome >= 200;
        const bool trace = diagnostic_pumps < 3;
        if (trace) ++diagnostic_pumps;
        if (trace) std::fprintf(stderr, "pump: begin\n");
        term_pump();
        bool visible_redraw = false;
        for (QWidget *root : page_roots_) {
            for (PtyView *v : views(root)) {
                const bool changed = v->poll_and_update(false);
                visible_redraw |= changed && v->isVisible();
            }
        }
        if (trace) std::fprintf(stderr, "pump: views done\n");
        if (docks_->shells) visible_redraw |= docks_->shells->poll(refresh);
        if (visible_redraw && !isMinimized()) {
            if (now - last_redraw > 32) dense_redraws = 1;
            else if (dense_redraws < 3) ++dense_redraws;
            if (dense_redraws == 3) fast_until = now + 128;
            last_redraw = now;
        }
        const int interval = now < fast_until ? 16 : 20;
        if (pump_->interval() != interval) {
            pump_->setTimerType(interval == 16 ? Qt::PreciseTimer : Qt::CoarseTimer);
            pump_->start(interval);
        }
        // SSH 交互提示（host key / kb）：泵驱动轮询（对话框模态阻塞泵时由
        // 对话框自身的 exec 循环接管 UI，应答经桥直接回 Rust）。
        poll_prompts(this);
        if (trace) std::fprintf(stderr, "pump: prompts done\n");
        // SFTP 浏览器：ops 结果分发 + 传输队列刷新（⑤）。
        if (docks_ != nullptr && docks_->sftp != nullptr) {
            docks_->sftp->poll();
        }
        if (trace) std::fprintf(stderr, "pump: sftp done\n");
        // AI 助手：生成结果领取（⑥）。
        if (docks_ != nullptr && docks_->ai != nullptr) {
            docks_->ai->poll();
        }
        if (trace) std::fprintf(stderr, "pump: ai done\n");
        if (refresh) {
            last_chrome = now;
            refresh_chrome();
        }
        if (now - last_status >= 1000) {
            last_status = now;
            chrome::tick_status_bar(this); // ⑧ fps/时钟（1s 节流）
        }
        if (trace) std::fprintf(stderr, "pump: end\n");
    });
    pump_->start(20);

    if (open_first_tab) add_tab();
    else show_welcome();
    PtyView *active = active_view();
    chrome::refresh_docks(docks_, active ? active->tab() : UINT32_MAX);
    chrome::install_auto_lock(this);
}

void MainWindow::add_tab() {
    add_tab_with(term_new_tab());
}

void MainWindow::add_tab_with(uint32_t tab_id) {
    dismiss_welcome();
    // Each page has a single-row session header (action strip + connection bar) and terminal
    // panes; its sender lives in the bottom dock.
    auto *root = new QWidget(this);
    root->setMinimumSize(0, 0);
    root->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Ignored);
    auto *lay = new QVBoxLayout(root);
    lay->setContentsMargins(0, 0, 0, 0);
    lay->setSpacing(0);

    auto *banner = new SessionBanner(root);
    // 常用动作图标条并入页头同一行（2026-10-01：WindTerm 单行页头样式）。
    banner->set_lead_widget(chrome::build_favorite_actions(this));
    auto *splitter = chrome::panel_splitter(Qt::Vertical, root);
    chrome::install_panel_split(splitter); // ⑬ 不可折叠 + 比例钳 0.1–0.9 + 面板手柄/比例持久化
    auto *sendbar = new SendBar(root);

    lay->addWidget(banner);
    lay->addWidget(splitter, 1);
    auto *footer = new QLabel(root);
    footer->setObjectName(QStringLiteral("sessionFooter"));
    footer->setTextFormat(Qt::PlainText);
    footer->setMinimumWidth(0);
    footer->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    footer->setMargin(3);
    lay->addWidget(footer);
    register_sendbar(sendbar);

    term_set_render_opts(tab_id, sess_active_config(tab_id).options.cursor_style, true);
    auto *v = new PtyView(tab_id, true);
    splitter->addWidget(v);

    banner->bind(v->tab());
    banner->set_actions(
        [this] {
            // ⋮ 窗格菜单：对齐 WindTerm 页头 ⋮ 实拍结构（2026-10-01 取证）——
            // 编辑三件套│保存会话│缩放│分割视图/放大窗格│全屏│页内标签操作。
            // 全部代理菜单栏同一 QAction（行为走 dispatch 单点，可用态同步）。
            QMenu m;
            chrome::style_menu(&m);
            struct Entry {
                const char *tid;
                const char *icon;
                const char *text;
            };
            static constexpr Entry kEntries[] = {
                {"edit:copy", "copy", "复制"},
                {"edit:paste", "paste", "粘贴"},
                {"edit:paste-selected", "paste", "粘贴选定的文本"},
                {nullptr, nullptr, nullptr},
                {"session:save", "save", "保存会话"},
                {nullptr, nullptr, nullptr},
                {"view:zoom", "zoom_text", "缩放"},
                {"window:split-view-horizontal", "split_h", "水平分割视图"},
                {"window:split-view-vertical", "split_v", "垂直分割视图"},
                {"window:zoom-pane", "zoom", "放大/还原窗格"},
                {nullptr, nullptr, nullptr},
                {"mode:fullscreen", "fullscreen", "全屏"},
            };
            for (const Entry &e : kEntries) {
                if (e.tid == nullptr) {
                    chrome::add_sep(&m);
                    continue;
                }
                chrome::proxy_action(&m, chrome::find_menu_action(menuBar(),
                                       QLatin1String(e.tid)),
                                     QString::fromLatin1(e.icon), i18n::tr(e.text));
            }
            chrome::add_sep(&m);
            chrome::add_action(&m, QStringLiteral("add"), i18n::tr("新建标签"), true,
                               [this] { add_tab(); });
            chrome::proxy_action(&m, chrome::find_menu_action(menuBar(),
                                 QStringLiteral("window:close-pane")),
                                 QStringLiteral("close"), i18n::tr("关闭窗格"));
            chrome::proxy_action(&m, chrome::find_menu_action(menuBar(),
                                 QStringLiteral("session:close-view")),
                                 QStringLiteral("close"), i18n::tr("关闭标签"));
            if (QAction *picked = m.exec(QCursor::pos())) {
                Q_UNUSED(picked);
            }
        },
        // ▶ 发送：展开并聚焦本页发送输入（发送栏）。
        [this, root] { focus_send_input(root); },
        // ⌄ 下拉：打开会话 / 最近打开（复用菜单栏同一批动作，不复制逻辑）。
        [this] {
            QMenu menu;
            chrome::style_menu(&menu);
            chrome::add_action(&menu, QStringLiteral("open"), i18n::tr("打开会话…"), true,
                               [this] { chrome::trigger_tid(this, "session:open"); });
            chrome::add_action(&menu, QStringLiteral("reopen"), i18n::tr("最近打开"), true,
                               [this] { chrome::trigger_tid(this, "session:recent"); });
            menu.exec(QCursor::pos());
        });

    configure_sendbar(sendbar, [this, root = QPointer<QWidget>(root)]() -> PtyView * {
        if (!root) return nullptr;
        PtyView *source = active_panes_.value(root);
        return source && is_ancestor(root, source) ? source : first_view(root);
    });
    page_roots_.push_back(root);    banners_.push_back(banner);
    sendbars_.push_back(sendbar);
    active_panes_[root] = v;
    const int at = tabs_->addTab(root, q_title(v->tab()));
    tabs_->setCurrentIndex(at);
    v->setFocus();
    refresh_chrome();
}

void MainWindow::close_tab(int i) {    if (i < 0 || i >= int(page_roots_.size())) {
        return;
    }
    QWidget *root = page_roots_[size_t(i)];
    if (!chrome::prepare_mark_close(this, views(root))) return;
    remember_closed_tab(root);
    auto *group = workspace_->owner(root);
    const int closed_index = group ? group->indexOf(root) : -1;
    active_panes_.remove(root);
    for (PtyView *v : views(root)) {
        v->mark_closed();          // 先封桥：deleteLater 排队期间不得再发 term_*
        term_close_tab(v->tab()); // 逐窗格终止会话
    }
    root->deleteLater();
    // 先理账再 removeTab，避免 currentChanged 访问已终止会话。
    page_roots_.erase(page_roots_.begin() + i);
    banners_.erase(banners_.begin() + i);
    remove_sendbar(sendbars_[size_t(i)]); sendbars_.erase(sendbars_.begin() + i);
    if (group) group->removeTab(group->indexOf(root));
    if (group == tabs_ && group->count() == 0) {
        for (auto *candidate : workspace_->groups()) if (candidate != group && candidate->count()) { tabs_ = candidate; break; }
    }
    workspace_->prune_empty(group);
    if (page_roots_.empty()) {
        show_welcome();
    } else {
        if (group == tabs_) tabs_->setCurrentIndex(qMax(0, closed_index - 1));
    }
    refresh_chrome();
}
// Group page reordering is owned by main_window_groups.cpp.

// ---- 窗格（Phase C ②；分割/关格/遍历/折叠在 main_window_panes.cpp） ----

// ---- chrome（Phase C ③） ----

void MainWindow::note(const QString &msg) {
    chrome::status_note(statusBar(), msg); // 两行状态栏：写行 1 左段，4s 回落「就绪」
}

void MainWindow::focus_pane_delta(int delta) {
    const int cur = current_page();
    if (cur < 0 || cur >= int(page_roots_.size())) {
        return;
    }
    auto vs = views(page_roots_[size_t(cur)]);
    if (vs.empty()) {
        return;
    }
    PtyView *active = active_view();
    int at = 0;
    for (int i = 0; i < int(vs.size()); ++i) {
        if (vs[size_t(i)] == active) {
            at = i;
        }
    }
    const int n = int(vs.size());
    vs[size_t(((at + delta) % n + n) % n)]->setFocus();
    refresh_status();
}

void MainWindow::toggle_zoom_active() {
    PtyView *v = active_view();
    if (!v) {
        return;
    }
    auto *ps = dynamic_cast<QSplitter *>(v->parentWidget());
    if (!ps || ps->count() < 2) {
        note(i18n::tr("单窗格页无需放大"));
        return;
    }
    if (zoomed_ == v && zoom_splitter_ == ps) {
        ps->setSizes(QList<int>(zoom_sizes_.begin(), zoom_sizes_.end()));
        zoom_splitter_ = nullptr;
        zoomed_ = nullptr;
        zoom_sizes_.clear();
    } else {
        zoom_sizes_ = std::vector<int>(ps->sizes().begin(), ps->sizes().end());
        QList<int> zeros(ps->count(), 0);
        zeros[ps->indexOf(v)] = 1;
        ps->setSizes(zeros);
        zoom_splitter_ = ps;
        zoomed_ = v;
    }
}

void MainWindow::focus_session(uint32_t id) {
    if (docks_->shells && docks_->shells->focus_session(id)) { refresh_chrome(); return; }
    for (int i = 0; i < int(page_roots_.size()); ++i) {
        for (PtyView *v : views(page_roots_[size_t(i)])) {
            if (v->tab() == id) {
                tabs_ = workspace_->owner(page_roots_[size_t(i)]);
                tabs_->setCurrentWidget(page_roots_[size_t(i)]);
                v->setFocus();
                refresh_chrome();
                return;
            }
        }
    }
    note(i18n::tr("会话 %1 不在当前窗口").arg(id));
}

// ---- 主题（Phase C ⑦） ----
// apply_theme() 定义在 main_window_layout.cpp（本文件已到 500 行硬限）。

// ---- 自检辅助 ----

std::vector<uint32_t> MainWindow::pane_ids(int page) const {
    std::vector<uint32_t> ids;
    if (page >= 0 && page < int(page_roots_.size())) {
        for (PtyView *v : views(const_cast<QWidget *>(page_roots_[size_t(page)]))) {
            ids.push_back(v->tab());
        }
    }
    return ids;
}

PtyView *MainWindow::pane_view(int page, int j) const {
    auto vs = views(const_cast<QWidget *>(page_roots_[size_t(page)]));
    return (j >= 0 && j < int(vs.size())) ? vs[size_t(j)] : nullptr;
}

SessionBanner *MainWindow::page_banner(int i) const {
    return (i >= 0 && i < int(banners_.size())) ? banners_[size_t(i)] : nullptr;
}

SendBar *MainWindow::page_sendbar(int i) const {
    return (i >= 0 && i < int(sendbars_.size())) ? sendbars_[size_t(i)] : nullptr;
}

uint32_t MainWindow::page_id(int i) const {
    if (i < 0 || i >= int(page_roots_.size())) {
        return UINT32_MAX;
    }
    auto vs = views(const_cast<QWidget *>(page_roots_[size_t(i)]));
    return vs.empty() ? UINT32_MAX : vs[0]->tab();
}

int MainWindow::page_count() const { return int(page_roots_.size()); }

void MainWindow::refresh_status() {
    chrome::tick_status_bar(this);
}

std::string MainWindow::tab_title(int i) const {
    if (i < 0 || i >= page_count()) return {};
    const uint32_t id = page_id(i);
    return id == UINT32_MAX ? std::string() : q_title(id).toUtf8().toStdString();
}

// ---- 私有（窗格树遍历/折叠在 main_window_panes.cpp） ----

void MainWindow::close_current_tab() {
    if (docks_->shells && docks_->shells->owns_focus()) {
        docks_->shells->close_shell(docks_->shells->currentIndex()); return;
    }
    close_tab(current_page());
}

void MainWindow::next_tab(int delta) {
    if (docks_->shells && docks_->shells->owns_focus()) {
        auto *shells = docks_->shells;
        if (shells->count()) shells->setCurrentIndex((shells->currentIndex() + delta + shells->count()) % shells->count());
        return;
    }
    const int n = tabs_->count();
    if (n > 0) {
        tabs_->setCurrentIndex((tabs_->currentIndex() + delta + n) % n);
    }
}
