// PodTreeModel 实现：DFS 拍平快照 → 层级树；QTreeView 数据源。

#include "cpp/chrome/pod_tree_model.h"
#include "cpp/i18n.h"

#include <QMimeData>

#include <functional>
#include <QString>

namespace {

PodNode make_node(const SessRow &r) {
    PodNode n;
    n.id = QString::fromUtf8(r.id.data(), int(r.id.size()));
    n.label = QString::fromUtf8(r.label.data(), int(r.label.size()));
    n.kind = r.kind;
    n.protocol = QString::fromUtf8(r.protocol.data(), int(r.protocol.size()));
    n.status = QString::fromUtf8(r.status.data(), int(r.status.size()));
    return n;
}

// Compare the view mirror before resetting: idle ticks must preserve indexes and selection.
bool same_snapshot(const PodNode &root, const rust::Vec<SessRow> &rows) {
    size_t at = 0;
    auto same_text = [](const QString &s, const rust::String &r) {
        return s == QString::fromUtf8(r.data(), int(r.size()));
    };
    std::function<bool(const PodNode &, uint32_t)> walk = [&](const PodNode &parent, uint32_t depth) {
        for (const auto &node : parent.children) {
            if (at >= rows.size()) return false;
            const auto &r = rows[at++];
            if (r.depth != depth || r.kind != node->kind || !same_text(node->id, r.id) ||
                !same_text(node->label, r.label) || !same_text(node->protocol, r.protocol) ||
                !same_text(node->status, r.status) || !walk(*node, depth + 1)) return false;
        }
        return true;
    };
    return walk(root, 0) && at == rows.size();
}

/// internalPointer 的安全回取：全部节点由 reload() 的树持有，索引生命周期即模型生命周期。
const PodNode *node_of(const QModelIndex &i) {
    return static_cast<const PodNode *>(i.internalPointer());
}

} // namespace

PodTreeModel::PodTreeModel(QObject *parent) : QAbstractItemModel(parent) {
    root_.label = QStringLiteral("TideTerm");
}

void PodTreeModel::reload(const rust::Vec<SessRow> &rows) {
    // 拖拽进行中：模型 reset 会让 QTreeView 的拖拽源索引悬空（参考 DOM 树不会在
    // drop 中途重建）。无 Qt 取消回调 ⇒ 5s 超时自愈。
    if (drag_clock_.isValid() && drag_clock_.elapsed() < 5000) {
        return;
    }
    if (same_snapshot(root_, rows)) return;
    beginResetModel();
    root_ = PodNode();
    root_.label = QStringLiteral("TideTerm");
    total_ = 0;
    // depth 栈：当前各层「正在收子节点」的节点（sessions.json 分组树可任意深）。
    // 栈深 = depth+1（栈底是虚根）。跳深异常行（数据源保证 DFS 恰深一层）防御丢弃。
    std::vector<PodNode *> stack{&root_};
    for (const SessRow &r : rows) {
        if (r.depth + 1 > stack.size()) {
            continue; // 跳深异常（数据源保证 DFS 恰深一层）——防御丢弃
        }
        stack.resize(r.depth + 1);
        PodNode *parent = stack.back();
        PodNode n = make_node(r);
        n.row = int(parent->children.size());
        n.parent = parent;
        parent->children.push_back(std::make_unique<PodNode>(std::move(n)));
        PodNode *placed = parent->children.back().get();
        ++total_;
        if (placed->kind == 0) {
            stack.push_back(placed); // 分组：后续更深行挂它下面
        }
    }
    endResetModel();
}

QModelIndex PodTreeModel::index(int row, int column, const QModelIndex &parent) const {
    if (row < 0 || column < 0) {
        return {};
    }
    const PodNode *p = parent.isValid() ? node_of(parent) : &root_;
    if (p == nullptr || row >= int(p->children.size())) {
        return {};
    }
    return createIndex(row, column, p->children[size_t(row)].get());
}

QModelIndex PodTreeModel::parent(const QModelIndex &child) const {
    if (!child.isValid()) {
        return {};
    }
    const PodNode *n = node_of(child);
    if (n == nullptr || n->parent == nullptr || n->parent == &root_) {
        return {};
    }
    return createIndex(n->parent->row, 0, n->parent);
}

int PodTreeModel::rowCount(const QModelIndex &parent) const {
    const PodNode *p = parent.isValid() ? node_of(parent) : &root_;
    return p ? int(p->children.size()) : 0;
}

int PodTreeModel::columnCount(const QModelIndex &) const {
    return 1; // 连接树单列（名称；协议/状态走角色，delegate ③ 不做）
}

QVariant PodTreeModel::data(const QModelIndex &index, int role) const {
    const PodNode *n = index.isValid() ? node_of(index) : nullptr;
    if (n == nullptr) {
        return {};
    }
    switch (role) {
        case Qt::DisplayRole:
            return n->label;
        case Qt::DecorationRole:
            // 分组/会话行图标只用 Microsoft YaHei UI 必含的 Geometric Shapes（■/□），
            // 与 WindTerm 行图标密度对齐；主题级图标落在 ⑦ QSS。罕见码位会触发 Qt
            // 全系统字体回退枚举（DirectWrite 映射上百 MB 字体），故不用 🖿/▣ 等。
            return QVariant(QString(n->kind == 0 ? QStringLiteral("■") : QStringLiteral("□")));
        case RoleProtocol:
            return n->protocol;
        case RoleStatus:
            return n->status;
        case RoleNodeId:
            return n->id;
        case RoleKind:
            return int(n->kind);
        default:
            return {};
    }
}

Qt::ItemFlags PodTreeModel::flags(const QModelIndex &index) const {
    if (!index.isValid()) {
        return Qt::NoItemFlags;
    }
    const PodNode *n = node_of(index);
    Qt::ItemFlags f = QAbstractItemModel::flags(index) | Qt::ItemIsSelectable |
                      Qt::ItemIsEnabled;
    if (n != nullptr && !n->id.startsWith(QStringLiteral("live:")) && !n->id.startsWith(QStringLiteral("saved:"))) f |= Qt::ItemIsDropEnabled;
    // 参考只允许拖「会话」（drag-drop.ts:4-8 的用途注释）；分组行只做落点。
    if (n != nullptr && n->kind == 1 && !n->id.startsWith(QStringLiteral("live:")) && !n->id.startsWith(QStringLiteral("saved:"))) {
        f |= Qt::ItemIsDragEnabled;
    }
    return f;
}

Qt::DropActions PodTreeModel::supportedDropActions() const {
    return Qt::MoveAction;
}

QStringList PodTreeModel::mimeTypes() const {
    return {QString::fromLatin1(kTreeDragMime)};
}

QMimeData *PodTreeModel::mimeData(const QModelIndexList &indexes) const {
    auto *mime = new QMimeData();
    for (const QModelIndex &i : indexes) {
        if (!i.isValid()) {
            continue;
        }
        const PodNode *n = node_of(i);
        if (n != nullptr && n->kind == 1) {
            mime->setData(QString::fromLatin1(kTreeDragMime), n->id.toUtf8());
            drag_clock_.restart(); // 起锁：拖拽期间不整表 reset
        }
        break; // 单选语义（参考一次只拖一条会话）
    }
    return mime;
}

bool PodTreeModel::canDropMimeData(const QMimeData *data, Qt::DropAction action, int, int,
                                   const QModelIndex &parent) const {
    if (action != Qt::MoveAction || data == nullptr || !data->hasFormat(QString::fromLatin1(kTreeDragMime))) {
        return false;
    }
    const PodNode *n = node_of(parent);
    return n != nullptr && !n->id.startsWith(QStringLiteral("saved:")) && n->id != QString::fromUtf8(data->data(QString::fromLatin1(kTreeDragMime)));
}

bool PodTreeModel::dropMimeData(const QMimeData *data, Qt::DropAction action, int row, int,
                                const QModelIndex &parent) {
    const QString mime = QString::fromLatin1(kTreeDragMime);
    if (!canDropMimeData(data, action, row, 0, parent)) {
        return false;
    }
    const QString dragged = QString::fromUtf8(data->data(mime));
    // Qt 的落点模型 = (row, parent)：row<0 ⇒ 「落在行上」= inside；
    // row>=0 ⇒ 插到该下标之前 = before（末下标之后视作 after 末行）。
    // 参考的 25%/75% 三带在 Qt 只有 50% 两带 + inside（§6 环境适配）。
    QString target;
    uint8_t pos = 2;
    if (row < 0) {
        target = parent.data(RoleNodeId).toString();
    } else {
        const int n = rowCount(parent);
        if (row >= n) {
            target = index(n - 1, 0, parent).data(RoleNodeId).toString();
            pos = 1;
        } else {
            target = index(row, 0, parent).data(RoleNodeId).toString();
            pos = 0;
        }
    }
    drag_clock_.invalidate(); // 先解锁，好让紧随其后的刷新能看到新树
    const rust::String err = sess_drag_move(
        rust::String(dragged.toUtf8().toStdString()),
        rust::String(target.toUtf8().toStdString()), pos);
    if (err.empty()) {
        if (on_note) {
            on_note(i18n::tr("已移动会话 %1").arg(dragged));
        }
        return true;
    }
    if (on_note) {
        on_note(QString::fromUtf8(err.data(), int(err.size())));
    }
    return false;
}
