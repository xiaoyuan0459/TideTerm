#pragma once

class MainWindow;
class QWidget;

namespace chrome {
/// 常用会话动作图标条（用户可从 ⋮ 增删）。装配进会话头（SessionBanner）左簇，
/// 与连接栏同一行（2026-10-01：WindTerm 单行页头样式；此前是窗口级独立工具栏行）。
QWidget *build_favorite_actions(MainWindow *window);
} // namespace chrome