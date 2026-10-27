// docks —— chrome 面板装配（Phase C ③ 骨架 → ④ 会话树真数据 → ⑤ SFTP）。
//
// 左：当前连接资源 + 当前终端文件；右：保存的会话树 + 当前终端命令历史。
#pragma once

class QMainWindow;
class QDockWidget;
class QListView;
class QStringListModel;
class QLabel;
class QPushButton;
class MainWindow;
class PodTreeModel;
class AiPanel;
class SftpBrowser;
class CommandHistoryView;
class ShellPanel;

#include <cstdint>
#include <utility>
#include <vector>

#include <QString>
#include <QStringList>

/// chrome 面板集合（MainWindow 持有；裸指针 + 无 qobject_cast —— ADR-014 结论）。
struct ChromeDocks {
    QDockWidget *conn = nullptr;   // 左上：资源管理器（当前连接）
    QDockWidget *files = nullptr;  // 左下：文件管理器（SFTP）
    QDockWidget *sess = nullptr;   // 右上：会话
    QDockWidget *hist = nullptr;   // 右下：会话日志
    /// Per-terminal bounded history supplied by Rust.
    QDockWidget *history = nullptr;
    QDockWidget *send = nullptr;
    QDockWidget *shell = nullptr;
    ShellPanel *shells = nullptr;
    QDockWidget *transfer = nullptr;
    CommandHistoryView *commands = nullptr;
    PodTreeModel *tree_model = nullptr;
    QListView *session_list = nullptr;
    QStringListModel *session_labels = nullptr;
    QStringListModel *log_labels = nullptr;
    QLabel *log_empty = nullptr;
    QPushButton *log_capture = nullptr;
    uint32_t log_tab = UINT32_MAX;
    uint64_t log_revision = UINT64_MAX;
    /// 连接树是否已做过首次展开（首刷全展开，之后按用户展开态对账）。
    bool tree_seen = false;
    /// 会话列行 → 稳定会话 id（与 labels 同步刷新）。
    std::vector<uint32_t> session_ids;
    /// 面板筛选串（WT 面板头筛选行）：连接树 / 会话 / 会话日志各一份。
    QString tree_filter;
    QString sess_filter;
    QString log_filter;
    /// 会话列全量（label, tab id）——筛选后重建 labels + session_ids，保证行号对齐。
    std::vector<std::pair<QString, uint32_t>> session_rows;
    /// 会话日志全量行（筛选前）。
    QStringList log_all;
    /// 文件管理器（⑤）：MainWindow 泵 poll + 活动会话切换 attach。
    SftpBrowser *sftp = nullptr;
    /// AI 助手面板（⑥）的**浮窗宿主**（不进四区；菜单按需打开，见 docks.cpp 布局段）。
    AiPanel *ai = nullptr;
    QDockWidget *bottom = nullptr;
};

namespace chrome {

/// 装配 WindTerm 工作台面板并挂到窗口（MainWindow ctor 调一次；返回堆对象由窗口持有）。
ChromeDocks *build_docks(MainWindow *win);
void build_log_dock(MainWindow *win, ChromeDocks *d);
void refresh_log_dock(ChromeDocks *d, uint32_t active_tab);
void build_auxiliary_docks(MainWindow *win, ChromeDocks *d);

/// 从 pod_*/sess_* 桥拉最新快照（泵的 ~1s 节流点调用）。
void refresh_docks(ChromeDocks *d, uint32_t active_tab);

/// 打开持久会话（树双击 / tree_menu「连接」共用）：sess_open → 新标签 + 最近记录。
void open_saved_session(MainWindow *win, const QString &sess_id);

} // namespace chrome
