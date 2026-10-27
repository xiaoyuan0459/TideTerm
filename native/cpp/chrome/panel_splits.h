// 面板分割（面板小窗内部的双栏/双行分割）。
//
// 语义与中央窗格分割**同一套**：`install_split_ratio` 的 0.1–0.9 比例钳 + 不可折叠。
// 面板再补三件中央区不需要的东西：
//   ① 手柄热区：面板比中央窗格窄得多，Qt 默认 1px 手柄在缩放下抓不住 ⇒ 5px 握持区，
//      视觉仍是居中的 1px 主题线（见 ui_metrics.h `kPanelSplitHandleWidth`）；
//   ② 悬停反馈：指针进入手柄时中央画一枚强调色握点（与 QSS `QSplitter::handle:hover`
//      同语义，但面板手柄是自绘件，不走 QSS）；
//   ③ 持久化：面板是"小窗"，用户拖过的比例要跨重启保留（中央窗格比例由
//      split_layout 自己的 QSettings 负责，不走这里）。
//
// 自绘件取色只走 `chrome::ui_colors()`（ui_theme.h），本 TU 不写死色值。
#pragma once

#include <Qt>
#include <QString>

class QSplitter;
class QWidget;

namespace chrome {

/// 手柄握持区宽度（px，逻辑像素）——见 ui_metrics.h `kPanelSplitHandleWidth`。
int panel_split_handle_width();

/// 纯函数：把 [0,total) 上的拖拽位置 `click` 换成 {上/左子, 下/右子} 尺寸。
/// 两个分量之和恒等于 `total`（total<=0 时返回 {0,0}）。
struct SplitClick {
    int upper;
    int lower;
};
SplitClick panel_split_states(int total, int click);

/// 面板分割器（自绘手柄 + 面板手柄宽度）。
QSplitter *panel_splitter(Qt::Orientation orientation, QWidget *parent = nullptr);

/// 装面板分割：比例钳 + 不可折叠，并登记进持久化表（见下）。
void install_panel_split(QSplitter *sp);

/// 面板分割比例的持久化（QSettings "TideTerm"/"TideTerm"，与 main_window_layout.cpp 同源）。
/// **每个分割器一个键**：`layout/panelSplits.v1/<类名>#<objectName>`，
/// 值 = `'h'|'v'` + 0x1f + `saveState().toHex()`（取向与比例一把存）。
/// 只覆盖经 `install_panel_split()` 登记过的分割器；上一轮写过而本轮不存在的键会被删除。
void save_panel_splits();
/// 恢复上一条保存的比例；取向已变的条目跳过（文件管理器会按宽度在竖排/横排间切换）。
void restore_panel_splits();

} // namespace chrome
