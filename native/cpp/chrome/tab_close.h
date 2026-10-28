// tab_close —— 标签关闭确认 + 中键关闭（对齐批次 10 · 清单 §3 标签条 / §4.7）。
//
// 参考 `TabBar/index.ts`：× 按钮（`:213`）与**中键**（`:181-187` auxclick）都走
// `requestClose` → `shouldConfirmClose`（`app.ts:810-816` **恒 true**，注释写明
// PRD §3.2 P0-F2 本意是「有进程在跑才确认」，v0.1 简化为总是确认）→
// `close-confirm.ts`（标题/正文/Enter 确认/Esc 取消/遮罩点击取消/确认后焦点归还）。
// 关窗格路径（`viewport-manager` → 最后一个窗格连带关标签）**不经确认**，
// 故确认只挂在「标签条发起的关闭」上，与参考一致（见 §9.7）。
#pragma once

#include <functional>
#include <QString>

class MainWindow;
class QTabBar;
class QWidget;

namespace chrome {

bool window_close_confirmation_enabled();
bool set_window_close_confirmation(bool enabled);
/// Confirm once for all terminal panes owned by this window; No/Esc is the default.
bool confirm_close_window(QWidget *parent, int sessions);

/// 关闭标签确认框；返回 true = 用户确认关闭。文案取参考 zh-CN `tabBar.*`。
bool confirm_close_tab(QWidget *parent, const QString &tab_label);

/// 给标签条装中键关闭过滤器（`on_close(视觉下标)`）。
void install_middle_click_close(QTabBar *bar, std::function<void(int)> on_close);

/// 标签条发起的关闭（× / 中键）：先确认，再 `MainWindow::close_tab`。
void request_close_tab(MainWindow *win, int index);

} // namespace chrome
