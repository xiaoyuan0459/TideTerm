// menu_style —— 弹出菜单的统一观感与装配助手（WT 对齐批次 4）。
//
// 背景：WT 的右键菜单（三张参考图）要素 = 14px 线性图标 + 图标槽 34 + 行高 27 +
// 分组分隔线 + 右侧快捷键文案 + 悬停高亮。QSS 只能管颜色/padding，管不到行高、
// 图标槽与「快捷键只显示不绑键」，所以这三件事在本模块装配期统一处理。
//
// **快捷键只显示不绑键**：做法是把快捷键拼进 QAction 文本（`"复制(C)\tShift+Del"`），
// 由 style 把 `\t` 后半段右对齐绘制。不用 `QAction::setShortcut()`——菜单栏里已有同键
// 动作，重复绑定会触发 Qt 的 ambiguous shortcut（两个都不响）。
//
// 动作复用：`proxy_action()` 拿菜单栏里**同一个** QAction（按 tid 递归查找）做代理，
// 触发即 `src->trigger()` ⇒ 行为仍走 menu_bar.cpp 的 dispatch 单点，不在本模块重写逻辑，
// 主菜单与代理菜单共用语义图标；没有匹配图形的动作留空，不按类别兜底。
#pragma once

#include <QString>

#include <functional>

class QAction;
class QMenu;
class QMenuBar;
class QWidget;
class MainWindow;

namespace chrome {

/// 弹出菜单统一观感（幂等）。装配期调一次。
void style_menu(QMenu *menu);

/// 菜单栏里按 `tid` 属性递归查找动作（menu_bar.cpp 的 id 表）。
QAction *find_menu_action(QMenuBar *bar, const QString &tid);

/// 触发菜单栏里 `tid` 对应的动作（找不到时 `note` 报不可用）。
/// 用途：装配次序早于 `build_menu()` 的常驻控件（如标签条快捷簇）——装配期取不到
/// QAction 指针，只能把解析推迟到点击时。行为仍走 menu_bar.cpp 的 dispatch 单点。
void trigger_tid(MainWindow *win, const char *tid);

/// 代理动作：图标 + 文本（可带 `\t` 快捷键）+ 与源动作同步的可用态；触发转调源动作。
/// `src` 为空时返回禁用项（诚实边界：没有对应能力就置灰）。
QAction *proxy_action(QMenu *menu, QAction *src, const QString &icon_name,
                      const QString &text);

/// 本模块自带回调的菜单项（树/文件管理器菜单那种不经菜单栏代理的项）。
/// `on_trigger` 为空表示「有这项但动作在别处接」（调用方 exec 后按指针分派）；
/// `enabled=false` 时置灰（缺后端）。
QAction *add_action(QMenu *menu, const QString &icon_name, const QString &text, bool enabled,
                    std::function<void()> on_trigger);

/// 分隔线（WT 的 1px 细分隔，间距由 QSS 给）。
void add_sep(QMenu *menu);

} // namespace chrome
