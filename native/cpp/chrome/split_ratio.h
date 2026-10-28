// 窗格分割比例钳位（对齐批次 13 · §3 中央）。
//
// 参考在拖把手时对二元比例做 `Math.max(0.1, Math.min(0.9, startRatio + delta/total))`
// （`panel-split-manager.ts:197`、`split-layout-controller.ts:194`），Qt 侧同形：
// 每个 `QSplitter` 只有两个子（`split_active` 造嵌套二元树），故钳「左/上子占比」。
//
// 只做**拖拽后修正**，不动子件 min/max：放大（zoom）靠 `setSizes({0,…,1,…})` 把兄弟
// 压到 0，一旦设过最小尺寸，放大就会静默失效。`splitterMoved` 只在用户拖动时发，
// 程序性 `setSizes` 不发 ⇒ 两条路天然分开。
#pragma once

class QSplitter;

namespace chrome {

/// 把子尺寸 a 钳进 [10%, 90%]（total<=0 或整页只有一个窗格时不钳）。
int clamp_split_a(int total, int a);

/// 就地钳位当前两子尺寸（供 `splitterMoved` 回调与自检直接调用）。
void apply_split_ratio_clamp(QSplitter *sp);

/// 装钳位：顺带 `setChildrenCollapsible(false)`（两个调用点都要求同一件事）。
void install_split_ratio(QSplitter *sp);

} // namespace chrome
