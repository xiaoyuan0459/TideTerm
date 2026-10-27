// UI 度量令牌（webview 版 `src/styles/main.css` `:root` 的 1:1 移植）。
//
// 分工铁律：**尺寸**只在本文件定义（C++ 侧 setFixedHeight/resizeDocks 用），
// **颜色与 padding/圆角**在 Rust `theme_qss.rs` 生成的 QSS 里定义。两处不得
// 重复声明同一个高度，否则主题切换与布局会互相漂移。
//
// 基准来源与逐项出处见 docs/reports/qt-webview-ui-parity-2026-09-18.md §2.1。
// 2026-09-25 起**骨架密度改以 WindTerm 实测值为准**（docs/reports/
// qt-windterm-ui-align-2026-09-25.md §2：本机缩放 1.2396 抓 client 像素量测），
// 注释里「WT 实测」即该表；未标注者仍是 webview 基准。
#ifndef TIDETERM_CHROME_UI_METRICS_H
#define TIDETERM_CHROME_UI_METRICS_H

namespace chrome::metrics {

// ---- 骨架五区（--menu-h / --status-h / --panel-w-left / --panel-w-right）----
/// WT 实测 31.5（物理 39）。
inline constexpr int kMenuBarHeight = 32;
inline constexpr int kWindowControlWidth = 40;
inline constexpr int kWindowControlGlyph = 10;
inline constexpr int kWindowResizeBand = 5;
inline constexpr int kFileScrollBarWidth = 10;
inline constexpr int kFileScrollHideDelay = 5000;
/// WT 状态栏是**两行**：物理 69 → 55.7，取 55 = 2×27（kStatusRowHeight）+ 1px 顶描边。
inline constexpr int kStatusBarHeight = 55;
/// 状态栏单行高（WT 物理 34 → 27.4）。
inline constexpr int kStatusRowHeight = 27;
/// 状态栏状态点（`.status-dot` 7×7 圆）与分隔条（`.status-sep` 1×11）。
inline constexpr int kStatusDotSize = 7;
inline constexpr int kStatusSepHeight = 11;
inline constexpr int kPanelWidthLeft = 310;
inline constexpr int kPanelWidthRight = 330;
/// 参考 `layout-manager.ts:58-59` 的拖拽钳位。
inline constexpr int kPanelWidthMin = 160;
inline constexpr int kPanelWidthMax = 480;

// ---- 标签条（center-area.css:11-194）----
/// WT 实测 29.9（物理 37）→ 取 30；单 tab 高由 QSS `QTabBar::tab{height}` 承载
/// （Qt 只在 QSS 里能量化单个 tab 项，故此处不重复声明，避免两处漂移）。
inline constexpr int kTabBarHeight = 30;
/// 窗格内边距（参考 center-area.css:443-455 `.terminal-pane-wrapper-col`
/// `padding: 8px 12px`，且 wrapper 是 content-box ⇒ 网格可用区 = 小窗 - 2*padding）。
inline constexpr int kPanePadX = 12;
inline constexpr int kPanePadY = 8;
// 活动窗格描边常量已删除（2026-10-01 用户要求移除终端外框；见 terminal_view.h paintEvent）。

inline constexpr int kTabMinWidth = 80;
inline constexpr int kTabMaxWidth = 220;
inline constexpr int kTabCloseButtonSize = 16;
inline constexpr int kTabLeadInset = 8;
inline constexpr int kTabCloseInset = 6;
/// 标签条右端快捷簇（WT `A` `+` `≡`）单钮边长：标签条 30 高，钮 18 + 上下各 6 留白。
inline constexpr int kTabQuickButtonSize = 18;
/// Local shell glyph (remote status uses the same stable leading slot).
inline constexpr int kTabActivitySize = 14;
inline constexpr int kTabRemoteStatusSize = 8;

// ---- 会话头（session-header.css:2-51）----
/// WT 会话工具行实测 25.8（物理 32）。
inline constexpr int kSessionHeaderHeight = 26;
inline constexpr int kSessionHeaderIconSize = 22;
/// 页头图标**字形**边长。2026-10-01 由 14 → 16：与 WindTerm 页头实拍放大对照，其图标
/// ≈16 逻辑 px、描边约 2 物理像素（旧 14px + 1px 描边明显偏细偏淡）；菜单图标仍 14。
inline constexpr int kSessionHeaderGlyphSize = 16;
/// 页头图标条按钮（并入连接栏同一行；行高 26 ⇒ 钮 22 = 上下各 2 留白）。
inline constexpr int kHeaderToolButtonSize = 22;

// ---- 侧栏节头与树行（side-panels.css:97-105 / :227-259 / :285-303）----
/// WT 面板头实测 25.8（物理 32）。
inline constexpr int kSectionHeaderHeight = 26;
/// 面板头左语义色块（WT 实测约 8 物理点）。
inline constexpr int kPanelDotSize = 8;
/// 面板头右侧 ⚙/✕ 钮（WT 约 18 逻辑）。
inline constexpr int kPanelHeaderButtonSize = 18;
/// WT 面板筛选行实测 21.8（物理 27）。
inline constexpr int kPanelFilterRowHeight = 22;
/// WT 树行实测 20.2（物理 25）。
inline constexpr int kTreeRowHeight = 21;
/// Resource rows include a 14px shell icon and breathing room at the current DPI.
inline constexpr int kResourceRowHeight = 22;
inline constexpr int kTreeIndentPerLevel = 16;
inline constexpr int kTreeRowBaseLeft = 8;
inline constexpr int kTreeStatusDotSize = 8;
/// 参考分组硬钳 3 层（sessionTree.ts:7）。

// ---- 底栏（bottom-tabs.css:2-46）----
inline constexpr int kBottomTabBarHeight = 24;
inline constexpr int kBottomTabHeight = 22;
/// 参考 `--bottom-panel-h` 默认 220，钳位 80–480（bottom-panel.ts:14-17）。
/// 2026-09-28 排布重做后常态内容 = 参数条 23 + 输入行 ≈ 30 + dock 标题 26，
/// 收到 88 避免输入区大片留白（用户可拖拽，钳位不变）。
inline constexpr int kBottomPanelHeight = 132;
inline constexpr int kBottomPanelMinHeight = 80;
inline constexpr int kBottomPanelMaxHeight = 480;
/// WT 「发送 ▾ | 文本(T) … 目标(T)」参数条实测 23.4（物理 29）。
inline constexpr int kSendParamRowHeight = 23;

// ---- 弹出菜单（WT 实测：行高 34 物理 → 27.4；图标 17 → 14）----
/// 菜单项行高（19 项相邻 ink 起点差一致 = 34 物理）。
inline constexpr int kMenuItemHeight = 27;
/// 菜单图标**字形**边长（物理 17）与「菜单左沿 → 文字」总槽宽（物理 42）。
inline constexpr int kMenuIconSize = 14;
/// 图标**画布**（字形居中画在里面）：QMenu 的行高 = 画布高 + 上下内边距，
/// 17 + 2×5 = 27 = WT 实测行高；直接用 14 的图会被 style 的 SmallIconSize 再缩一次。
inline constexpr int kMenuIconCanvas = 17;
inline constexpr int kMenuIconGutter = 34;
/// 跨分隔线只比普通行距多 3 物理 px（≈2.4 逻辑），故分隔条自身留白极小。
inline constexpr int kMenuSeparatorGap = 1;
/// 终端右键菜单的色板行圆点（物理 10）。
inline constexpr int kMenuPaletteDotSize = 8;

// ---- 分屏（split-layout-helpers.ts:16-18）----
inline constexpr double kSplitRatioMin = 0.1;
inline constexpr double kSplitRatioMax = 0.9;
/// 面板小窗内部分割的**握持区**宽度（2026-10-05）。中央窗格手柄沿用 Qt 默认 1px
/// （WindTerm 实拍同为 1 物理像素细线），但面板宽 220–480，1px 热区在高 DPI 下抓不住；
/// 故面板手柄取 5px 握持区 + 自绘居中的 1px 主题线，视觉线宽与中央区一致。
inline constexpr int kPanelSplitHandleWidth = 5;
/// 手柄悬停时中央的强调色握点长度（3px 在 1× 下也看得见，按住后可辨）。
inline constexpr int kPanelSplitGripSize = 3;

// ---- 菜单栏右侧工具钮（menu-bar.css:47-90）----
inline constexpr int kMenuToolButtonSize = 24;

} // namespace chrome::metrics

#endif // TIDETERM_CHROME_UI_METRICS_H
