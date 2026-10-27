// term_menu —— 终端右键菜单（WT 对齐批次 4）。
//
// 项序与分组照 WT 实测（截图 3：19 项 + 5 条分隔线 + 7 色色板行）。落点铁律：
// **能接真操作的接、缺后端的置灰**（`.rules` 的「菜单要接真实操作或置灰」）。
// 常规动作代理菜单栏里的同 tid QAction；命令动作固定右键落点的来源 ID，
// 执行前重新核对保留状态，避免打开菜单后改动另一条命令或另一个窗格。
//
// 高亮入口复用实际规则管理；触发器和完整重置子菜单仍置灰。
// 选择命令块包含提示符及输出，不提供未实现的参考快捷键。
#pragma once
#include <cstdint>

class QMenu;
class QWidget;

namespace chrome {

/// 组装终端右键菜单（不 exec；caller 决定 exec 或量测）。返回堆对象，caller 负责释放。
/// `context` 是弹出点的控件——内部用 `dynamic_cast` 沿 `window()` 找 MainWindow
/// （Qt 侧无 moc，不能用 qobject_cast）；找不到时仍出菜单，全部项置灰。
QMenu *build_term_menu(QWidget *context, uint64_t command = 0);

} // namespace chrome
