// MainWindow 的布局状态与主题应用（Phase C 对齐批次 A/B）。
//
// 单独成 TU 的原因：`main_window.cpp` 已 499/500 行（`check-file-size.sh` 硬限），
// 成员函数的定义可分散在同类的多个 TU 中，无需改动既有编排代码。
//
// 持久化口径（2026-09-18 定案，见 docs/reports/qt-webview-ui-parity-2026-09-18.md §5）：
// 视图几何属视图态，走 QSettings，**不进** 与 Slint 壳同源共享的 UiPrefs；
// 键名沿用 webview 版 localStorage 的语义（panelWidths / splitLayout / …）。

#include <QApplication>
#include <QCloseEvent>
#include <QMenuBar>
#include <QSettings>
#include <QFileInfo>
#include <QStatusBar>
#include <QTabBar>
#include <QTabWidget>
#include <QTimer>
#include <set>

#include "cpp/bridge_shim.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/panel_splits.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/main_window.h"
#include "cpp/chrome/text_marks.h"
#include <QMessageBox>
#include <QElapsedTimer>
#include <QThread>
#include <QApplication>
#include "cpp/terminal/terminal_view.h"

namespace {

QSettings layout_settings() {
    return QSettings(QSettings::defaultFormat(), QSettings::UserScope,
                     QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
}

const char kKeyDockState[] = "layout/dockState.v4";
const char kKeyWindowGeom[] = "layout/windowGeometry.v1";

} // namespace

// ---- 主题（Phase C ⑦；进程级 QSS） ----
//
// qApp 同时覆盖主窗口、子控件和无父弹出件，避免在主窗口重复安装同一套规则。
void MainWindow::apply_theme() {
    const rust::String qss = prefs_qss();
    const QString sheet = QString::fromUtf8(qss.data(), int(qss.size()));
    qApp->setStyleSheet(sheet);
    chrome::refresh_ui_colors(); // 自绘件（delegate/tab 角标）取色缓存
    chrome::clear_icon_cache();  // 菜单矢量图标按主题前景色画，换主题后必须重画
    chrome::refresh_button_icons(this);
    // 终端正文 16 色随主题（与 chrome 同一信源），并请求重绘已画帧。
    term_apply_palette();
    for (PtyView *v : MainWindow::views(this)) {
        v->update();
    }
}

// ---- 布局状态 ----

QTabBar *MainWindow::tabs_bar() const { return tabs_->tabBar(); }

void MainWindow::restore_layout() {
    // 五区固定高度（WT 实测：菜单 31 / 状态栏 54=2×27；main.css:222-223 原为 36/24）。
    menuBar()->setFixedHeight(chrome::metrics::kMenuBarHeight);
    statusBar()->setFixedHeight(chrome::metrics::kStatusBarHeight);
    // 标签条 30（WT 实测物理 37）；单 tab 度量在 QSS 里按 objectName `termTabs` 限定，
    // 这样叠放 dock 的标签条不会被中央标签条的 min-width 130 撑爆。
    if (tabs_ != nullptr) {
        tabs_->setObjectName(QStringLiteral("termTabs"));
    }
    if (tabs_->tabBar() != nullptr) {
        tabs_->tabBar()->setFixedHeight(chrome::metrics::kTabBarHeight);
    }
    QSettings s = layout_settings();
    const QByteArray state = s.value(QLatin1String(kKeyDockState)).toByteArray();
    if (!state.isEmpty()) {
        restoreState(state);
    }
    const QByteArray geom = s.value(QLatin1String(kKeyWindowGeom)).toByteArray();
    if (!geom.isEmpty()) {
        restoreGeometry(geom);
    }
    if (!s.value(QStringLiteral("layout/windtermDensity.v1"), false).toBool()) {
        QTimer::singleShot(0, this, [this] {
            resizeDocks({docks_->conn, docks_->sess},
                {chrome::metrics::kPanelWidthLeft, chrome::metrics::kPanelWidthRight}, Qt::Horizontal);
        });
    }
    // 面板内部（文件管理器双栏 / 终端页窗格）的分割比例：等第一次布局跑完再恢复，
    // 此时面板才有真实尺寸；否则 restoreState 的比例会被随后的初始布局覆盖。
    QTimer::singleShot(0, this, [] { chrome::restore_panel_splits(); });
}

void MainWindow::save_layout() {
    QSettings s = layout_settings();
    s.setValue(QLatin1String(kKeyDockState), saveState());
    s.setValue(QLatin1String(kKeyWindowGeom), saveGeometry());
    s.setValue(QStringLiteral("layout/windtermDensity.v1"), true);
    chrome::save_panel_splits(); // 面板分割比例（QSettings 同一存储域，见 panel_splits.h）
}

void MainWindow::closeEvent(QCloseEvent *e) {
    if (close_prompt_active_) { e->ignore(); return; }
    std::set<uint32_t> sessions;
    for (QWidget *root : page_roots_)
        for (PtyView *view : views(root))
            if (!view->is_closed()) sessions.insert(view->tab());
    if (!sessions.empty()) {
        close_prompt_active_ = true;
        const bool confirmed = chrome::confirm_close_window(this, int(sessions.size()));
        close_prompt_active_ = false;
        if (!confirmed) { e->ignore(); return; }
    }
    if (!page_roots_.empty() && !save_workspace() && QMessageBox::question(this,
        i18n::tr("保存失败"), i18n::tr("工作区未保存，仍然关闭窗口？"),
        QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes) { e->ignore(); return; }
    if (!chrome::prepare_mark_close(this, views(this))) { e->ignore(); return; }
    for (PtyView *view : views(this)) if (!view->is_closed()) slog_stop(view->tab());
    QElapsedTimer drain; drain.start();
    setEnabled(false);
    while (slog_pending_drains() && drain.elapsed() < 5000) {
        QApplication::processEvents(QEventLoop::ExcludeUserInputEvents, 20); QThread::msleep(5);
    }
    setEnabled(true);
    if (slog_pending_drains()) {
        e->ignore(); note(i18n::tr("日志仍在写入，请稍后关闭窗口")); return;
    }
    save_layout();
    QMainWindow::closeEvent(e);
    if (e->isAccepted()) retire_sessions();
}

MainWindow::~MainWindow() { retire_sessions(); }

void MainWindow::retire_sessions() {
    halt_pump_for_probe();
    term_sync_close(sync_owner_); sync_owner_ = 0;
    std::vector<PtyView *> owned = views(this);
    for (QWidget *root : page_roots_) {
        const auto panes = views(root);
        owned.insert(owned.end(), panes.begin(), panes.end());
    }
    std::set<uint32_t> sessions;
    for (PtyView *view : owned) {
        if (view->is_closed()) continue;
        view->mark_closed(); // Deferred paints must stop before Rust retires the host.
        sessions.insert(view->tab());
    }
    for (uint32_t id : sessions) term_close_tab(id);
}

int MainWindow::current_page() const {
    return tabs_ == nullptr ? -1 : page_index(tabs_->currentWidget());
}

// 内存探针（--memory-probe --tab-nopump）：停掉 20 ms 会话泵，把「泵内桥调用 + 每秒
// chrome/docks 刷新」的常驻成本与控件树本身分开量。
void MainWindow::halt_pump_for_probe() {
    if (pump_ != nullptr) {
        pump_->stop();
    }
}

QString MainWindow::q_title(uint32_t id) {
    const rust::String value = term_title(id);
    const QString title = QString::fromUtf8(value.data(), int(value.size()));
    return title.endsWith(QLatin1String(".exe"), Qt::CaseInsensitive)
               ? QFileInfo(title).completeBaseName() : title;
}
