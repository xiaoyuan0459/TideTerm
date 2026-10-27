// SFTP 文件树列装配与列宽（Q-01 修复：构造期 resizeSection 的宽度到首次显示
// 时会被冲掉——面板先以极窄尺寸布局；此处在首次显示时重放一次意图宽度）。
// 头文件内联实现：无 moc、不进 build.rs 的 cpp 清单。
#pragma once

#include <QHeaderView>
#include <QStringList>
#include <QTreeWidget>

#include "cpp/i18n.h"

namespace sftp {

/// WT 对齐的意图列宽：名称 280 / 修改时间 150 / 类型 90 / 大小 80，
/// 远程另有权限 96 / 所有者 80 / 分组 80（2026-09-29 实测口径）。
inline void apply_initial_column_widths(QTreeWidget *tree, bool remote) {
    if (tree == nullptr || tree->columnCount() == 0) {
        return;
    }
    static const int kWidths[] = {280, 150, 90, 80, 96, 80, 80};
    const int n = sizeof(kWidths) / sizeof(kWidths[0]);
    for (int i = 0; i < tree->columnCount() && i < n; ++i) {
        tree->header()->resizeSection(i, kWidths[i]);
    }
    Q_UNUSED(remote); // 列数由 setColumnCount 决定，宽度表天然多退少停
}

/// 构造期一次性列装配：标签 / 交互模式 / 最小节宽 / 意图宽度。
/// WT 实测（2026-09-29）：名称列保宽（窄面板下其余列横向溢出滚动），
/// 本地 4 列、远程 7 列；时间列 150px 容纳 yyyy/MM/dd HH:mm:ss。
inline void setup_file_tree_columns(QTreeWidget *tree, bool remote) {
    if (tree == nullptr) {
        return;
    }
    tree->setColumnCount(remote ? 7 : 4);
    tree->setHeaderLabels(remote
            ? QStringList{i18n::tr("名称"), i18n::tr("修改时间"),
                          i18n::tr("类型"), i18n::tr("大小"), i18n::tr("权限"),
                          i18n::tr("所有者"), i18n::tr("分组")}
            : QStringList{i18n::tr("名称"), i18n::tr("修改时间"),
                          i18n::tr("类型"), i18n::tr("大小")});
    tree->header()->setSectionResizeMode(QHeaderView::Interactive);
    tree->header()->setStretchLastSection(false);
    tree->header()->setMinimumSectionSize(48); // 时间/大小列不可被压到不可读
    apply_initial_column_widths(tree, remote);
}

} // namespace sftp
