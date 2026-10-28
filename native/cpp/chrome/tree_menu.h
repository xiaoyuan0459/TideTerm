// tree_menu —— 连接树右键菜单八项（对齐批次 8 · 清单 §4.1）。
//
// 参考 `src/components/SessionTree/index.ts:366-376`：连接 / 编辑 / 重命名 / 复制 /
// 移动到分组 / 导出 / ─ / 删除；分组行无菜单（`:71-79` 原样保留 Qt 早退），
// 树空白区右键 = 直接触发导入（`:129-135`，参考不弹菜单）。
//
// 拆成「取输入」（弹模态框）与「执行」（`tree_action`，纯桥调用）两层：
// 自检不能驱动模态框，故直接断言 `tree_action`。
#pragma once

#include <QModelIndex>
#include <QString>

class MainWindow;

namespace chrome {

/// 会话节点右键菜单：收集输入后转 [`tree_action`]。`idx` 无效/分组 = 空操作。
void show_tree_menu(MainWindow *win, const QModelIndex &idx);

/// 会话**视图**右键菜单（WT 对齐批次 4，项序照参考截图 5）：挂在「会话」dock 的活跃会话列，
/// 是视图/标签级操作（关闭视图、拆分视图、复制会话 URL…）。`tab_id` 无效时空操作。
void show_view_menu(MainWindow *win, uint32_t tab_id);

/// 树空白区右键 = 选文件并导入（参考无菜单、直接 onImport）。
void tree_import_by_prompt(MainWindow *win);

/// 执行一项树操作；`arg` = 已由对话框收集好的输入（新名/分组路径/文件路径）。
/// 返回给用户看的提示（空串 = 按参考语义「无事发生」，如名称未变）。
/// key：connect / rename / duplicate / move / export / import / favorite / delete。
QString tree_action(MainWindow *win, const QString &id, const QString &key,
                    const QString &arg = QString());

} // namespace chrome
