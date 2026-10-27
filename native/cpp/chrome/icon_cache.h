// icon_cache —— 右键菜单矢量图标 + 进程级缓存（WT 对齐批次 4）。
//
// 为什么不用图片/资源：native 是 cargo 单入口（无 rcc / 无 .qrc / 无 Qt Svg），
// 引入二进制图标要么加资源编译管线，要么加图片解码依赖。这里按用户裁决走
// **QPainter 矢量自绘 + 静态缓存**：
//   · 菜单、按钮与标签 QIcon 按实际尺寸/DPI 懒绘制；
//   · 位图走有界 QPixmapCache，不预留未使用的 DPI 档；
//   · 按 devicePixelRatio 分键，高 DPI 下不糊。
// 描边色取主题（`ui_colors().text_secondary`），换主题时 `clear_icon_cache()` 重画
// （MainWindow::apply_theme 里与 refresh_ui_colors 同批调用）。
//
// WT 实测口径：菜单图标 14 逻辑 px（物理 17），图标槽 34（见 ui_metrics.h）。
#pragma once

#include <cstdint>

#include <QIcon>
#include <QString>
#include <QColor>

class QAbstractButton;
class QWidget;
class QAction;

namespace chrome {

/// 图标描边墨色角色：页头（工具栏/连接栏）用主文字色——WindTerm 参考里该行图标与地址
/// 同为亮色（2026-10-01 实拍对照）；其余控件维持次级色。按**角色**而非固定色值传递，
/// 换主题后 refresh_button_icons 才能解析到新主题的颜色。
enum class IconInk { Secondary, Primary, Success };

/// Protocol symbols shared by saved sessions, live sessions and connection menus.
void set_action_icon(QAction *action, const QString &id);
QString protocol_icon(const QString &protocol);
QString protocol_icon_for_kind(uint8_t kind);

/// 取图标（按绘制设备 DPI 懒创建 + 缓存）。
QIcon scalable_icon(const QString &name, int size, const QColor &ink = QColor());
/// 装按钮图标（默认次级色；页头传 IconInk::Primary）。
void set_button_icon(QAbstractButton *button, const QString &name, int size = 14,
                     IconInk ink = IconInk::Secondary);
void refresh_button_icons(QWidget *root);

/// 换主题后清缓存（图标颜色随主题，必须重画）。
void clear_icon_cache();

} // namespace chrome
