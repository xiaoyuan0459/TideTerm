// MainWindow —— Phase C ①②③：多标签 + 嵌套分割 + chrome（菜单/dock/横幅/发送栏）。
//
// 结构：QMainWindow → 工作台面板 + TabWorkspace 嵌套标签组
// 每组独立 QTabWidget（可关/movable/跨组拖动）→ 每标签一个页根 QWidget
// 【SessionBanner + QSplitter（分割可任意深嵌套）】→ PtyView；SendBar 在全宽底部按页切换。
// 业务状态全在 Rust（ADR-012 铁律 2）；菜单/横幅/发送栏经 chrome:: 模块装配。
//
// 无 Q_OBJECT/moc：信号槽用 lambda 连接，窗格遍历用 dynamic_cast
// （AUTOMOC 保持触发式引入，见 ADR-014）。
#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

#include <QMainWindow>
#include <QHash>
#include <QPointer>
#include <QString>
#include <QWidget>
#include <QByteArray>
#include <QJsonObject>

#include "cpp/chrome/split_ratio.h"
#include "cpp/i18n.h"

class PtyView;
class QSplitter;
class QTabWidget;
class TabWorkspace;
class QTimer;
class QStackedWidget;
class SendBar;
class SessionBanner;
struct ChromeDocks;

class MainWindow : public QMainWindow {
public:
    explicit MainWindow(bool open_first_tab = true);
    ~MainWindow() override;

    // ---- 标签 ----
    void add_tab();
    /// 为 Rust 侧已存在的会话 id 建 UI 页（Phase C ④：sess_open/quick_open 后）。
    void add_tab_with(uint32_t id);
    void close_tab(int i);
    void close_current_tab();
    void split_group(Qt::Orientation orientation, bool clone_session = false);
    void merge_current_group();
    void close_current_group();
    void activate_group(QTabWidget *group);
    int page_index(QWidget *root) const;
    void sync_group_order(QTabWidget *group);
    TabWorkspace *workspace() const { return workspace_; }
    /// 会话标题（OSC 标题，去 .exe 后缀；chrome 控件如面包屑兜底也用）。
    static QString q_title(uint32_t id);

    // ---- 窗格（Phase C ②） ----
    /// 收集页内全部终端窗格（树遍历；无 moc ⇒ dynamic_cast）。
    static std::vector<PtyView *> views(QWidget *root);
    /// 当前焦点窗格（无焦点——offscreen/启动期——回落到当前页第一个窗格）。
    PtyView *active_view() const;
    /// 分割当前焦点窗格：新窗格与它并列（原生 QSplitter 嵌套，任意深）。
    void split_active(Qt::Orientation o);
    /// 关闭窗格：终止其会话；页内最后一个窗格关闭 = 关整页。
    void close_pane(PtyView *v);

    // ---- chrome（Phase C ③） ----
    /// chrome 数据面（docks/menu_bar 模块用）。
    ChromeDocks *docks() { return docks_; }
    /// 状态栏反馈（菜单/发送栏/横幅的统一出口）。
    void note(const QString &msg);
    /// 显示并聚焦某页的发送输入（连接栏 ▶「发送命令行」：展开底部发送区并聚焦输入框）。
    void focus_send_input(QWidget *page_root);
    void show_completion_sources(bool collect = false);
    void new_sender_window();
    void show_sync_channels();
    bool update_sync_scope();
    uint64_t sync_owner() const { return sync_owner_; }
    std::vector<uint32_t> send_target_ids(uint32_t source, int scope) const;
    /// 焦点轮转窗格（activate-next/previous）。
    void focus_pane_delta(int delta);
    /// 放大/还原当前窗格（zoom-pane：暂存 splitter 尺寸，其余压 0）。
    void toggle_zoom_active();
    /// 聚焦持有会话 id 的窗格（dock 双击：连接树/会话列 → 跳标签+聚焦）。
    void focus_session(uint32_t id);

    // ---- 主题（Phase C ⑦） ----
    /// 读取当前偏好（ui_prefs）并应用全局 QSS（ctor 首次 + 首选项保存后）。
    void apply_theme();

    // ---- 自检辅助 ----
    /// 第 i 页第 j 窗格的稳定 id。
    std::vector<uint32_t> pane_ids(int page) const;
    /// 第 i 页第 j 窗格视图。
    PtyView *pane_view(int page, int j) const;
    /// 第 i 页横幅/发送栏（c3 断言用）。
    SessionBanner *page_banner(int i) const;
    SendBar *page_sendbar(int i) const;
    /// 第 i 个标签的稳定 id = 页首窗格的会话 id（关闭其他标签后**位置≠id**）。
    uint32_t page_id(int i) const;
    int page_count() const;
    /// 当前标签页索引（无标签 -1；状态栏/自检取「活动页」用）。
    int current_page() const;

    void refresh_chrome();
    void refresh_status();
    /// 第 i 页标题（自检）。
    std::string tab_title(int i) const;
    /// 「关闭标签是否需要确认」策略（默认弹框；自检替换）。参考 app.ts:810-816 恒 true。
    std::function<bool(const QString &)> close_confirm_ask;
    /// 中央标签条（度量自检用）。
    class QTabBar *tabs_bar() const;
    /// 内存探针专用：停掉 20 ms 会话泵，把「泵内桥调用 + 每秒 chrome/docks 刷新」的常驻
    /// 成本与控件树本身分开量（`--memory-probe --tab-nopump`）。QTimer 在本头文件里只是
    /// 前置声明，定义在 main_window_layout.cpp。
    void halt_pump_for_probe();

    /// 无会话时展示起始页；会话创建后自动移除。
    void show_welcome();
    void dismiss_welcome();
    void reopen_closed_tab();
    bool save_workspace();
    void choose_workspace();
    bool restore_previous_workspace();
    bool restore_workspace(const QByteArray &snapshot);

protected:
    /// 确认关窗后保存布局并回收本窗会话；取消保持会话与草稿。
    void closeEvent(QCloseEvent *e) override;

private:
    void build_send_panel();
    void register_sendbar(SendBar *bar);
    void remove_sendbar(SendBar *bar);
    void sync_send_panel();
    void configure_sendbar(SendBar *bar, std::function<PtyView *()> source);
    void restore_layout();
    void save_layout();
    void retire_sessions();
    QByteArray capture_workspace(QWidget *only_page = nullptr) const;
    QJsonObject capture_page(QWidget *root, std::vector<uint32_t> *ids) const;
    QWidget *restore_page(const QJsonObject &page, const QByteArray &snapshot);
    void remember_closed_tab(QWidget *root);
    QString workspace_key_;
    std::vector<QByteArray> closed_tabs_;

    static void collect(QWidget *w, std::vector<PtyView *> *out);
    static bool is_ancestor(QWidget *root, QWidget *w);
    static PtyView *first_view(QWidget *root);
    static void collapse_if_single(QSplitter *ps);
    void next_tab(int delta);
    /// 页根内取分割器（页根第 2 子；add_tab 保证结构）。
    QSplitter *page_splitter(QWidget *root) const;

    TabWorkspace *workspace_ = nullptr;
    QTabWidget *tabs_ = nullptr;
    QStackedWidget *send_stack_ = nullptr;
    QWidget *welcome_ = nullptr;
    QTimer *pump_ = nullptr;
    ChromeDocks *docks_ = nullptr;
    std::vector<QWidget *> page_roots_;
    /// 与 page_roots_ 同序（无 moc：qobject_cast/findChildren 不可用，直接持指针）。
    std::vector<SessionBanner *> banners_;
    std::vector<SendBar *> sendbars_;
    QHash<QWidget *, QPointer<PtyView>> active_panes_;
    uint32_t files_tab_ = UINT32_MAX;
    QString files_uid_;
    bool close_prompt_active_ = false;
    uint64_t sync_owner_ = 0;
    // zoom-pane 暂存（无 moc：裸指针成员足够，窗格关闭时置空守卫）。
    QSplitter *zoom_splitter_ = nullptr;
    PtyView *zoomed_ = nullptr;
    std::vector<int> zoom_sizes_;
};
