// menu_bar —— 菜单完整化（Phase C ③）：11 顶级菜单，结构/文案/快捷键对齐
// native 侧 menu_model.rs（canonical；id 一致 ⇒ 语义跨壳对齐）。
//
// QAction 一元化：每个动作恰好一处 connect → dispatch()（对比 Slint 壳的
// dispatch_menu/dispatch_shell_menu 双分发铁律——Qt 侧单点天然消除同步坑）。
// 未接线动作统一落状态栏「未实现」反馈（诚实边界），不静默。
#pragma once

class MainWindow;

namespace chrome {

/// 构建完整菜单栏（MainWindow ctor 调一次）。
void build_menu(MainWindow *win);

} // namespace chrome
