// PodTreeModel —— 连接树模型（Phase C ③；QAbstractItemModel 子类）。
//
// 无 Q_OBJECT/moc（ADR-014 §Decision 2：覆写 data()/index()/rowCount() 并调用
// 继承的 dataChanged 不需要元对象；代价是 qobject_cast 不可用，调用方直接持指针）。
//
// 数据：Rust `pod_rows()` 的 DFS 拍平快照（分组在前、子紧随 + depth），此处按
// depth 栈重建层级。状态在 Rust（铁律 2）——刷新 = 整表 reset（骨架规模下开销
// 可忽略；④ 接 sessions.json 后再评估增量对账）。
#pragma once

#include <QAbstractItemModel>
#include <QElapsedTimer>

#include <cstdint>
#include <functional>
#include <memory>
#include <vector>

#include "cpp/bridge_shim.h"

/// 拖拽载体格式：值 = 被拖会话 id（分组行不可拖，见 flags()）。
inline constexpr char kTreeDragMime[] = "application/x-tideterm-session-id";

/// 树节点（C++ 侧仅结构镜像，不含业务逻辑）。children 持 unique_ptr——节点
/// 堆地址稳定，父指针/internalPointer 不随 vector 扩容失效。
struct PodNode {
    QString id;
    QString label;
    uint8_t kind = 1; // 0=分组 1=会话
    QString protocol;
    QString status;
    std::vector<std::unique_ptr<PodNode>> children;
    int row = 0; // 在父 children 中的下标（index() 生成用）
    PodNode *parent = nullptr;
};

class PodTreeModel : public QAbstractItemModel {
public:
    enum Role {
        RoleProtocol = Qt::UserRole + 1,
        RoleStatus = Qt::UserRole + 2,
        RoleNodeId = Qt::UserRole + 3,
        /// 0=分组 1=会话（delegate 决定是否画状态点）。
        RoleKind = Qt::UserRole + 4,
    };

    explicit PodTreeModel(QObject *parent = nullptr);

    /// 从 Rust 快照重建（DFS 拍平 → depth 栈组装层级；rust::Vec = cxx 载体）。
    /// Phase C ④ 起：数据源 = session 域 `sess_tree_rows`（sessions.json 分组树）。
    void reload(const rust::Vec<SessRow> &rows);

    // ---- QAbstractItemModel（无 moc：全部覆写虚函数） ----
    QModelIndex index(int row, int column,
                      const QModelIndex &parent = QModelIndex()) const override;
    QModelIndex parent(const QModelIndex &child) const override;
    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    int columnCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role = Qt::DisplayRole) const override;
    Qt::ItemFlags flags(const QModelIndex &index) const override;

    // ---- 拖拽改组 / 组内排序（对齐批次 9 · §4.1）----
    Qt::DropActions supportedDropActions() const override;
    QStringList mimeTypes() const override;
    QMimeData *mimeData(const QModelIndexList &indexes) const override;
    bool canDropMimeData(const QMimeData *data, Qt::DropAction action, int row, int column,
                         const QModelIndex &parent) const override;
    bool dropMimeData(const QMimeData *data, Qt::DropAction action, int row, int column,
                      const QModelIndex &parent) override;

    /// 自检辅助：节点总数（含根层分组）。
    int total_nodes() const { return total_; }

    /// 拖放结果的提示回调（docks 装配时指向 MainWindow::note；空 = 静默）。
    std::function<void(const QString &)> on_note;

private:
    PodNode root_; // 虚根：children 即顶层节点
    int total_ = 0;
    /// 拖拽期间的模型锁：reload() 的 beginResetModel 会销毁拖拽源 QModelIndex。
    /// 无取消回调 ⇒ 5s 后自动失效（超时后下一次刷新照常）。
    mutable QElapsedTimer drag_clock_;
};
