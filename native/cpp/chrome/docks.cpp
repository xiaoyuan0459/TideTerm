// WindTerm panel assembly: live resources, saved sessions, files and commands.
#include "cpp/chrome/docks.h"
#include "cpp/chrome/command_history_view.h"
#include "cpp/chrome/live_session_labels.h"
#include "cpp/chrome/display_names.h"
#include "cpp/chrome/panel_controls.h"

#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/prompts.h"
#include "cpp/chrome/ai_panel.h"
#include "cpp/chrome/menu_actions.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/chrome/tree_delegate.h"
#include "cpp/chrome/tree_menu.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

#include <QDesktopServices>
#include <QDockWidget>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListView>
#include <QMenu>
#include <QPushButton>
#include <QSizePolicy>
#include <QStyledItemDelegate>
#include <QStringListModel>
#include <QTreeView>
#include <QUrl>
#include <QVBoxLayout>

namespace {

class CompactListView : public QListView {
public:
    using QListView::QListView;
    QSize sizeHint() const override { return {230, 300}; }
    QSize minimumSizeHint() const override { return {160, 80}; }
};

class ResourceItemDelegate final : public QStyledItemDelegate {
public:
    using QStyledItemDelegate::QStyledItemDelegate;
    QSize sizeHint(const QStyleOptionViewItem &option, const QModelIndex &index) const override {
        QSize size = QStyledItemDelegate::sizeHint(option, index);
        size.setHeight(qMax(chrome::metrics::kResourceRowHeight, option.fontMetrics.height() + 6));
        return size;
    }
};

/// dock 头（标题栏）样式对齐 WindTerm 密度：标题 + 折叠角默认原生。
QDockWidget *make_dock(MainWindow *win, const QString &title, const QString &object_name,
                       QWidget *body) {
    auto *d = new QDockWidget(title, win);
    d->setObjectName(object_name);
    d->setWidget(body);
    d->setFeatures(QDockWidget::DockWidgetMovable | QDockWidget::DockWidgetClosable | QDockWidget::DockWidgetFloatable);
    return d;
}

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 连接树过滤（WT 面板头筛选行）：命中行 + 其祖先链保留，其余 setRowHidden。
/// 返回「本子树是否有命中」（needle 为空时恒 true ⇒ 全显）。
bool apply_tree_filter(QTreeView *view, const QAbstractItemModel *model,
                       const QModelIndex &parent, const QString &needle) {
    bool any = false;
    const int rows = model->rowCount(parent);
    for (int r = 0; r < rows; ++r) {
        const QModelIndex idx = model->index(r, 0, parent);
        const bool child_hit = apply_tree_filter(view, model, idx, needle);
        const bool self_hit =
            idx.data(Qt::DisplayRole).toString().contains(needle, Qt::CaseInsensitive);
        const bool keep = self_hit || child_hit;
        view->setRowHidden(r, parent, !keep);
        any = any || keep;
    }
    return any;
}

} // namespace

namespace chrome {

/// 会话列：按 `sess_filter` 重建 labels + session_ids（行号必须与 ids 对齐，
/// 否则双击会跳错标签——见 main_window 的 close_tab 同类教训）。
void apply_session_filter(ChromeDocks *d) {
    if (d == nullptr || d->session_labels == nullptr) {
        return;
    }
    QStringList labels;
    d->session_ids.clear();
    for (const auto &row : d->session_rows) {
        if (!d->sess_filter.isEmpty() &&
            !row.first.contains(d->sess_filter, Qt::CaseInsensitive)) {
            continue;
        }
        labels << row.first;
        d->session_ids.push_back(row.second);
    }
    static_cast<LiveSessionLabels *>(d->session_labels)->visible_ids = d->session_ids;
    if (d->session_labels->stringList() != labels) d->session_labels->setStringList(labels);
}

} // namespace chrome

namespace chrome {

/// 打开持久会话（树双击 / 右键「连接」/ 表单「立即连接」共用）：sess_open → 新标签。
void open_saved_session(MainWindow *win, const QString &sess_id) {
    if (sess_id.startsWith(QStringLiteral("live:"))) {
        bool valid = false;
        const auto id = sess_id.mid(5).toUInt(&valid);
        if (valid) win->focus_session(id);
        return;
    }
    const uint32_t tab = chrome::sess_open_with_vault(win, rs(sess_id));
    if (tab == chrome::kVaultLocked) {
        win->note(i18n::tr("已取消：凭据库未解锁，未自动填充记住的密码"));
        return;
    }
    if (tab == UINT32_MAX) {
        win->note(i18n::tr("会话不存在：%1").arg(sess_id));
        return;
    }
    win->add_tab_with(tab);
    chrome::record_recent(sess_id);
    win->note(i18n::tr("已打开会话 %1").arg(sess_id));
}

} // namespace chrome

namespace chrome {

ChromeDocks *build_docks(MainWindow *win) {
    auto *d = new ChromeDocks;

    // ---- 保存的会话：QTreeView + PodTreeModel，数据来自 sessions.json ----
    d->tree_model = new PodTreeModel(win);
    auto *tree = new QTreeView(win);
    tree->setItemDelegate(new chrome::TreeDelegate(tree));
    tree->setModel(d->tree_model);
    tree->setHeaderHidden(true);
    tree->setRootIsDecorated(true);
    // WT 实测（2026-10-06）：组行图标距面板边 ~22px、组→子行一层缩进 ~24px；
    // 默认 indent(20)+行内边距叠出 46/54px。压到 12 后：层差 ≈28px。
    tree->setIndentation(12);
    tree->setUniformRowHeights(true);
    // 拖拽改组/排序（批次 9）：DragDrop + 自研 mime，落点解释与执行都在 Rust。
    tree->setDragEnabled(true);
    tree->setAcceptDrops(true);
    tree->setDropIndicatorShown(true);
    tree->setDragDropMode(QAbstractItemView::DragDrop);
    tree->setDefaultDropAction(Qt::MoveAction);
    tree->setSelectionBehavior(QAbstractItemView::SelectRows);
    tree->setSelectionMode(QAbstractItemView::SingleSelection);
    d->tree_model->on_note = [win](const QString &msg) { win->note(msg); };
    QObject::connect(
        tree, &QTreeView::doubleClicked, win, [win](const QModelIndex &idx) {
            const QVariant v = idx.data(PodTreeModel::RoleNodeId);
            if (!v.isValid()) {
                return;
            }
            const QString sess_id = v.toString();
            if (idx.data(PodTreeModel::RoleKind).toInt() == 1 && !sess_id.isEmpty()) {
                open_saved_session(win, sess_id);
            }
        });
    tree->setContextMenuPolicy(Qt::CustomContextMenu);
    QObject::connect(
        tree, &QTreeView::customContextMenuRequested, win,
        [win, tree](const QPoint &pos) {
            const QModelIndex hit = tree->indexAt(pos);
            // 参考 index.ts:129-135：空白区右键不弹菜单，直接触发导入。
            if (hit.data(PodTreeModel::RoleNodeId).toString().startsWith(QStringLiteral("saved:"))) return;
            if (hit.isValid() && !hit.data(PodTreeModel::RoleNodeId).isNull()) {
                chrome::show_tree_menu(win, hit);
            } else {
                chrome::tree_import_by_prompt(win);
            }
        });
    d->sess = make_dock(win, i18n::tr("会话"), QStringLiteral("dockSess"), tree);
    {
        // WT 面板头 + 筛选行；过滤只走 setRowHidden（不动模型/proxy，避开拖拽改组
        // 与 reload 的索引失效——`C9-x` 覆盖拖入分组链路）。
        PanelHeader *head = chrome::install_panel_header(d->sess, i18n::tr("会话"),
                                                        i18n::tr("筛选"));
        if (head != nullptr) {
            head->set_dot(QStringLiteral("accent"));
            head->on_filter = [d, tree](const QString &text) {
                d->tree_filter = text;
                apply_tree_filter(tree, tree->model(), QModelIndex(), text);
            };
        }
    }

    // ---- 左下：文件管理器（⑤：SftpBrowser——本地栏即扫；远程栏随活动 SSH 会话） ----
    d->sftp = new SftpBrowser(win);
    d->sftp->on_note = [win](const QString &msg) { win->note(msg); };
    d->files = make_dock(win, i18n::tr("文件管理器"), QStringLiteral("dockFiles"),
                         d->sftp);
    // User requested a file manager without a filter field.
    chrome::install_panel_header(d->files, i18n::tr("文件管理器"))
        ->set_dot(QStringLiteral("accent")); // WT：文件管理器 = 蓝

    // ---- 当前连接：双击聚焦实际终端 ----
    d->session_labels = new LiveSessionLabels(win);
    d->session_list = new CompactListView(win);
    d->session_list->setObjectName(QStringLiteral("resourceList"));
    d->session_list->setItemDelegate(new ResourceItemDelegate(d->session_list));
    d->session_list->setIconSize(QSize(14, 14));
    d->session_list->setSpacing(0);
    d->session_list->setUniformItemSizes(true);
    d->session_list->setWordWrap(false);
    d->session_list->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    d->session_list->setTextElideMode(Qt::ElideRight);
    d->session_list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    d->session_list->setModel(d->session_labels);
    d->session_list->setEditTriggers(QAbstractItemView::NoEditTriggers);
    QObject::connect(d->session_list, &QListView::doubleClicked, win,
                     [d, win](const QModelIndex &idx) {
                         const int row = idx.row();
                         if (row >= 0 && row < int(d->session_ids.size())) {
                             win->focus_session(d->session_ids[size_t(row)]);
                         }
                     });
    // 视图级右键菜单（WT 对齐批次 4）：项序照参考截图 5，挂在活跃会话列上。
    d->session_list->setContextMenuPolicy(Qt::CustomContextMenu);
    QObject::connect(d->session_list, &QListView::customContextMenuRequested, win,
                     [d, win](const QPoint &pos) {
                         const QModelIndex hit = d->session_list->indexAt(pos);
                         if (!hit.isValid()) {
                             return;
                         }
                         const int row = hit.row();
                         if (row >= 0 && row < int(d->session_ids.size())) {
                             chrome::show_view_menu(win, d->session_ids[size_t(row)]);
                         }
                     });
    d->conn = make_dock(win, i18n::tr("资源管理器"), QStringLiteral("dockConn"),
                        d->session_list);
    if (PanelHeader *head = chrome::install_panel_header(d->conn, i18n::tr("资源管理器"),
                                                         i18n::tr("筛选"))) {
        head->set_dot(QStringLiteral("danger"));
        head->on_filter = [d](const QString &text) {
            d->sess_filter = text;
            apply_session_filter(d);
        };
    }

    build_log_dock(win, d);

    // ---- AI 面板宿主：独立浮窗，不占底栏（2026-09-26 用户裁决：底栏不放 AI UI） ----
    d->bottom = make_dock(win, i18n::tr("AI 助手"), QStringLiteral("dockAi"),
                          nullptr);
    d->bottom->hide(); // 默认不出现（用户要求底栏与可视图面里没有 AI）

    d->commands = new CommandHistoryView(win);
    d->history = make_dock(win, i18n::tr("历史命令"), QStringLiteral("dockHistory"), d->commands);
    auto *history_head = chrome::install_panel_header(d->history, i18n::tr("历史命令"), i18n::tr("筛选"));
    history_head->set_dot(QStringLiteral("info"));
    history_head->on_filter = [d](const QString &text) { d->commands->set_filter(text); };
    build_auxiliary_docks(win, d);

    // 左：资源/文件；右：保存的会话/历史命令；日志默认隐藏。
    win->addDockWidget(Qt::LeftDockWidgetArea, d->conn);
    win->splitDockWidget(d->conn, d->files, Qt::Vertical);
    win->addDockWidget(Qt::RightDockWidgetArea, d->sess);
    win->splitDockWidget(d->sess, d->history, Qt::Vertical);
    win->addDockWidget(Qt::RightDockWidgetArea, d->hist);
    d->hist->hide();
    d->history->raise();
    // AI 面板宿主**不进四区**：先 addDockWidget 让 Qt 接管生命周期与 saveState 兼容，
    // 随即 removeDockWidget 退出布局 ⇒ 底栏不再有 AI 条，它只在被菜单打开时以浮窗出现。
    win->addDockWidget(Qt::BottomDockWidgetArea, d->bottom);
    win->removeDockWidget(d->bottom);
    d->bottom->setFloating(true);
    d->bottom->resize(560, 320); // 浮窗默认尺寸（不进布局后没有父容器给尺寸）
    // 浮窗同样要自绘面板头：没有它，AI 助手就是唯一一个顶着 Qt 原生标题栏的裸浮窗
    // （其余面板都有 panelHeader），拖动、主题、关闭钮都不一致。
    PanelHeader *ai_head = chrome::install_panel_header(d->bottom, i18n::tr("AI 助手"));
    ai_head->set_dot(QStringLiteral("accent"));
    QObject::connect(d->bottom, &QDockWidget::visibilityChanged, win,
                     [win](bool visible) { if (visible) ensure_ai_panel(win); });
    // 初始布局 = webview 版度量（main.css:224-225：左 224 / 右 256），
    // 拖拽钳位 160–480（layout-manager.ts:58-59）；用户仍可拖动 dock 边界。
    for (QDockWidget *side : {d->conn, d->files}) {
        side->setMinimumWidth(chrome::metrics::kPanelWidthMin);
        side->setMaximumWidth(chrome::metrics::kPanelWidthMax);
    }
    for (QDockWidget *side : {d->sess, d->history, d->hist}) {
        side->setMinimumWidth(chrome::metrics::kPanelWidthMin);
        side->setMaximumWidth(chrome::metrics::kPanelWidthMax);
    }
    win->resizeDocks({d->conn, d->sess},
                     {chrome::metrics::kPanelWidthLeft, chrome::metrics::kPanelWidthRight},
                     Qt::Horizontal);
    win->resizeDocks({d->conn, d->files}, {1, 1}, Qt::Vertical);
    win->resizeDocks({d->sess, d->history}, {1, 1}, Qt::Vertical);
    // 文件管理器保留可操作的最小高度（右列已 tab 叠放，不再纵向分割）。
    d->files->widget()->setMinimumHeight(120);
    return d;
}

namespace {

/// 按节点 id 找索引（PodTreeModel 每次 reload 重建 QModelIndex，故只能用 id 定位）。
QModelIndex index_by_node_id(PodTreeModel *m, const QString &id,
                             const QModelIndex &parent = {}) {
    if (id.isEmpty()) {
        return {};
    }
    for (int r = 0; r < m->rowCount(parent); ++r) {
        const QModelIndex i = m->index(r, 0, parent);
        if (i.data(PodTreeModel::RoleNodeId).toString() == id) {
            return i;
        }
        const QModelIndex hit = index_by_node_id(m, id, i);
        if (hit.isValid()) {
            return hit;
        }
    }
    return {};
}

/// 收集展开中的分组 id（reload 后回灌，替代无条件 expandAll）。
void collect_expanded(PodTreeModel *m, QTreeView *tree, QStringList *out,
                      const QModelIndex &parent = {}) {
    for (int r = 0; r < m->rowCount(parent); ++r) {
        const QModelIndex i = m->index(r, 0, parent);
        if (tree->isExpanded(i)) {
            out->push_back(i.data(PodTreeModel::RoleNodeId).toString());
        }
        collect_expanded(m, tree, out, i);
    }
}

} // namespace

void refresh_docks(ChromeDocks *d, uint32_t active_tab) {
    if (d == nullptr) {
        return;
    }
    auto *tree = qobject_cast<QTreeView *>(d->sess->widget());
    // 参考 SessionTree 在数据刷新时保留展开态（sessionTree/SessionTree index.ts:59-100）；
    // Qt 侧整表 reset 同样会丢选中，故 reload 前后按节点 id 对账。
    QStringList expanded_before;
    QString selected_id;
    if (tree != nullptr) {
        collect_expanded(d->tree_model, tree, &expanded_before);
        selected_id = tree->currentIndex().data(PodTreeModel::RoleNodeId).toString();
        if (active_tab != UINT32_MAX) {
            const auto cfg = sess_active_config(active_tab);
            const QString active_id = q(cfg.id);
            selected_id = active_id.isEmpty() ? QStringLiteral("live:%1").arg(active_tab) : active_id;
        }
    }
    d->tree_model->reload(sess_explorer_rows());
    if (tree != nullptr) {
        if (!d->tree_seen) {
            d->tree_seen = true;
            tree->expandAll(); // 首刷全展开（此后按用户态回灌）
        }
        for (const QString &id : expanded_before) {
            const QModelIndex i = index_by_node_id(d->tree_model, id);
            if (i.isValid()) {
                tree->setExpanded(i, true);
            }
        }
        const QModelIndex keep = index_by_node_id(d->tree_model, selected_id);
        if (keep.isValid()) {
            tree->setCurrentIndex(keep);
        }
        // reload 的 reset 会丢弃 setRowHidden 状态，筛选需重放。
        if (!d->tree_filter.isEmpty()) {
            apply_tree_filter(tree, tree->model(), QModelIndex(), d->tree_filter);
        }
    }

    const rust::Vec<PodRow> sessions = pod_sessions();
    d->session_rows.clear();
    auto *live_model = static_cast<LiveSessionLabels *>(d->session_labels);
    QHash<uint32_t, QString> icons;
    QHash<uint32_t, uint8_t> states;
    for (const PodRow &r : sessions) {
        QString label = QString::fromUtf8(r.label.data(), int(r.label.size()));
        if (label.endsWith(QLatin1String(".exe"), Qt::CaseInsensitive)) {
            label = QFileInfo(label).completeBaseName();
        }
        bool ok = false;
        const uint32_t id = QString::fromUtf8(r.id.data(), int(r.id.size())).toUInt(&ok);
        if (ok) {
            icons.insert(id, chrome::session_icon(sess_active_config(id)));
            states.insert(id, term_status_kind(id));
        }
        d->session_rows.push_back({label, ok ? id : UINT32_MAX});
    }
    const bool appearance_changed = live_model->icons != icons || live_model->states != states;
    live_model->icons.swap(icons);
    live_model->states.swap(states);
    apply_session_filter(d); // labels 与 session_ids 必须同源重建（行号对齐）
    if (appearance_changed && live_model->rowCount() > 0) {
        emit live_model->dataChanged(live_model->index(0),
            live_model->index(live_model->rowCount() - 1), {Qt::DecorationRole, Qt::ForegroundRole});
    }
    for (int row = 0; row < int(d->session_ids.size()); ++row) {
        if (d->session_ids[size_t(row)] == active_tab) {
            d->session_list->setCurrentIndex(d->session_labels->index(row));
            break;
        }
    }
    d->commands->refresh(active_tab);
    refresh_log_dock(d, active_tab);
}

} // namespace chrome
