// SftpBrowser —— SFTP 双栏浏览器（Phase C ⑤ 真数据版；G3 系数试点重写）。
//
// 功能面对照 native/ui/sftp_browser.slint：路径栏×2（远程/本地 + 转到/上级）·
// 工具条 · 双栏树（名称/大小/权限/修改）· 右键菜单（新建目录/重命名/删除/
// 上传/下载）· 跨栏拖拽（上传/下载）· 传输队列。
//
// 数据流（④ 事件驱动同构）：attach(uid) → sftp_open → poll() 收 kind=0 结果
// → sftp_list(cwd)；目录操作异步发起、poll 领取结果；传输队列按版本号刷新。
// 无 Q_OBJECT/moc（std::function 回调；构建面省 moc——系数记录的一部分）。
#pragma once

#include <QSet>
#include <QWidget>
#include <QHash>
#include <QElapsedTimer>

#include <functional>
#include <QStringList>

class QLabel;
class QLineEdit;
class QResizeEvent;
class QSplitter;
class QToolBar;
class QTreeWidget;
class QTreeWidgetItem;
class QWidget;

/// 单栏（路径栏 + 文件树）——构造装配用的小聚合。
struct SftpPane {
    QWidget *box = nullptr;
    QWidget *path_strip = nullptr;
    QLineEdit *path = nullptr;
    QTreeWidget *tree = nullptr;
};

/// SFTP 浏览器（dock 内嵌部件）。
class SftpBrowser : public QWidget {
public:
    explicit SftpBrowser(QWidget *parent = nullptr);
    ~SftpBrowser() override;
    void follow_terminal(uint32_t tab, uint8_t kind, const QString &cwd, bool sftp_enabled = true, bool follow_cwd = true);
    QSize sizeHint() const override { return {280, 300}; }
    QSize minimumSizeHint() const override { return {220, 150}; }

    /// 绑定会话（uid = term 注册表会话 uid；title 显示用）。非 SSH 会话在
    /// open 阶段失败并显示错误。
    void attach(const QString &uid, const QString &title, const QString &initial_dir = QString());
    void detach(bool close_channel = false);

    /// 泵轮询（MainWindow 20ms tick 调）：ops 结果分发 + 传输队列刷新。
    void poll();
    QWidget *transfer_widget();

    /// 状态反馈出口（docks 注入到 MainWindow::note）。
    std::function<void(const QString &)> on_note;
    std::function<void()> on_transfer_started;

    // ---- 自检 ----
    int local_rows() const;
    int remote_rows() const;
    int transfer_rows() const;
    QString status_text() const;
    QString attached_uid() const;
    QTreeWidget *remote_tree() const;
    QTreeWidget *local_tree() const;
    QToolBar *toolbar_widget() const;
    /// 状态行统计（WT 实测口径）：无选中 `N 项[ 总大小]`，选中 `k/N 项[ 选中/总]`。
    QString stats_text(QTreeWidget *tree) const;
    /// 按当前可见栏刷新状态行（list 完成/选中变化时调用）。
    void refresh_stats();
    /// 自检/外层注入：直接列出本地目录（attach 前的本地栏填充）。
    void scan_local(const QString &dir);
    /// SFTP-05：外部文件（资源管理器拖入）的目标语义 —— 远程栏 = 逐个上传到
    /// `remote_cwd_`，本地栏 = 复制进 `local_cwd_`（同名不覆盖）。FileTree 的
    /// dropEvent 连线调它；公开给自检直测语义（合成 QDropEvent 在 offscreen
    /// 下不派发到 ItemView 的 dropEvent，走不到那条投递路径）。
    void handle_external_drop(const QStringList &files, bool to_remote);

protected:
    void resizeEvent(QResizeEvent *event) override;

private:
    void build_pane(SftpPane *p, bool remote);
    void build_toolbar();
    void update_connection_view();
    void setup_transfer_actions();
    void refresh_transfers();
    void poll_local();
    void remote_navigate(const QString &path);
    void do_context_menu(bool remote, const QPoint &pos);
    void start_transfer(bool upload);
    void enqueue_uploads(const QStringList &files);
    void run_file_job(uint8_t op, const QString &source_uid, const QString &source,
                      const QString &target_uid, const QString &target, uint8_t policy = 0);
    void copy_tree(const QString &source_uid, const QString &source, const QString &target_uid, const QString &target);
    void copy_selected(const QString &uid, const QStringList &sources);
    void move_selected(const QString &uid, const QStringList &sources);
    void insert_path(uint32_t tab, const QString &path);
    void edit_remote(uint64_t edit, const QString &path);

    QString uid_;
    QString remote_cwd_ = QStringLiteral("/");
    QString local_cwd_;
    QString status_;
    bool pending_open_ = false;
    quint64 transfers_seen_ = 0;
    QSet<QString> notified_done_; // 已见过的成功完成传输（刷新去重）
    bool transfers_pinned_ = false; // ⚙「传输面板」常驻开关（WT Ctrl+T 对齐）
    void set_transfers_pinned(bool pinned);
    void update_transfers_visibility();
    QElapsedTimer transfers_refresh_clock_;
    QToolBar *toolbar_ = nullptr;
    QLabel *empty_ = nullptr;
    QWidget *remote_box_ = nullptr;
    QWidget *local_box_ = nullptr;
    QWidget *remote_path_strip_ = nullptr;
    QWidget *local_path_strip_ = nullptr;
    bool browse_local_ = true;
    bool terminal_local_ = true;
    bool session_sftp_disabled_ = false;
    uint32_t context_tab_ = UINT32_MAX;
    QString context_cwd_;
    QHash<QString, QString> remote_paths_;
    uint64_t files_owner_ = 0;
    QSplitter *split_ = nullptr;
    QTreeWidget *transfers_ = nullptr;
    QHash<QString, QTreeWidgetItem *> transfer_items_;
    QLabel *status_label_ = nullptr;
    // 双栏（FileTree——cpp 内部类型；构造后经基类指针持有，回调在构造期接好）。
    QTreeWidget *remote_tree_ = nullptr;
    QTreeWidget *local_tree_ = nullptr;
    QLineEdit *remote_path_ = nullptr;
    QLineEdit *local_path_ = nullptr;
};

/// G3 系数试点兼容包装（离屏构建 + 桩行填充；g3-selftest 保留）。
QWidget *build_sftp_browser(QTreeWidget **out_tree, QToolBar **out_toolbar);
