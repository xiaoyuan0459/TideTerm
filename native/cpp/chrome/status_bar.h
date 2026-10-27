// 状态栏（Phase C ⑧；2026-09-25 WT 对齐批次 2 改为**两行**）：
//   行 1 左 = 「就绪」/ 临时消息（`status_note`）、右 = fps · 后端 · 主题（我方取证字段）；
//   行 2 右对齐 = 状态点 · 远端模式 · 窗口 W×H · 行 N · 字符 N · UTF-8 · 时间。
// 实测口径见 docs/reports/qt-windterm-ui-align-2026-09-25.md §2.1。
// FPS = 各窗格 painted() 计数的每秒增量（不钩 paintEvent，tick 读数即可）。
#pragma once

#include <QString>

class MainWindow;
class QStatusBar;

namespace chrome {

/// 组装两行字段（ctor 一次；label/分隔条以 objectName 供 tick 查找）。
void build_status_bar(MainWindow *win);

/// 每秒刷新状态点/结构字段/fps/时钟/主题（pump 1s 节流点调；多窗口各自持有 label）。
void tick_status_bar(MainWindow *win);

/// 临时状态消息（`MainWindow::note` 的唯一出口）：写行 1 左段，4s 后回落「就绪」。
/// 之所以不直接用 `QStatusBar::showMessage`：两行内容装在一个 permanent widget 里，
/// 会盖住 QStatusBar 自绘的临时消息区（见 status_bar.cpp 顶部注释）。
void status_note(QStatusBar *bar, const QString &msg);

} // namespace chrome
