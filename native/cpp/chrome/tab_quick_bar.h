// tab_quick_bar —— 标签条右端快捷簇（WT 对齐 2026-09-25）。
//
// 参考物：WT 标签条右端三钮（抓图 native/target/u-tabbar.png）。语义按用户
// 2026-09-26 提供的 WT 实拍修正：
//   `A` = **拆分组**（字母是分组编号：WT 里同一窗口多分组会显示 A/B/C）。点开是
//         水平拆分组 / 垂直拆分组 / 合并到分组 / 关闭分组。我方每页 = 一个分组
//         （无多分组模型），故字母恒为 A；项文本与可用态直接取自菜单栏的同一动作。
//   `+` = 新建标签（本机 shell，即 `MainWindow::add_tab()`）
//   `≡` = 标签页列表 + 连接动作（新建会话 / 快速连接 / 打开会话 / 最近打开 /
//         本机终端 / 逐标签跳转 / 关闭当前·其他标签）
//
// **动作延后解析**：本簇在 `MainWindow` ctor 里、早于 `chrome::build_menu()` 装配，
// 因此装配期拿不到菜单栏 QAction 指针；分组菜单在每次 `aboutToShow` 时按 `tid`
// 取**同一个** QAction 做代理（`menu_style::proxy_action`），触发即 `src->trigger()`
// ⇒ 行为仍走 `menu_bar.cpp`/`menu_actions.cpp` 的 dispatch 单点，本模块不复制业务逻辑。
//
// 无 Q_OBJECT/moc：QToolButton + lambda（ADR-014）。
#pragma once

#include <functional>

class MainWindow;
class QMenu;
class QTabWidget;
class QWidget;

namespace chrome {

/// 装配标签条右端快捷簇；返回值交给 `QTabWidget::setCornerWidget`。
QWidget *build_tab_quick_bar(MainWindow *win, QTabWidget *tabs);

/// 填充「新建会话」协议菜单：本机终端 / SSH / Telnet / Serial / TCP（WT 页头 □⁺▾
/// 同款，2026-10-01 实拍取证）。标签栏 + 与页头常用动作条共用，协议逻辑单点维护。
/// 每项新建前先调 `activate` 把目标分组置前（两处调用方各自传入）。
void fill_new_session_menu(QMenu *menu, MainWindow *win, const std::function<void()> &activate);

} // namespace chrome
