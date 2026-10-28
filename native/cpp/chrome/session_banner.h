// SessionBanner —— 页级会话横幅（页头）：2026-10-01 按 WindTerm 页头实拍重做为**单行**。
//
// 版式：[常用动作图标条][ⓘ] │ [▶] 协议 ▸ 分组 ▸ 会话名 ……… [>] [⌄] [⋮]
//   · 图标条由 MainWindow 经 set_lead_widget 注入（常用会话动作，可增删）
//   · ⓘ 会话信息（对话框）
//   · ▶ 发送：显示并聚焦本页发送输入（发送栏）
//   · 面包屑**分段可点击**（2026-10-02 WT 取证：会话段点击弹同级会话列表）：
//       协议段=新建会话协议菜单｜分组段=该分组会话列表｜会话段=同级会话列表
//   · > 转到：快速连接（输入新地址）｜⌄ 打开会话 / 最近打开（菜单）｜⋮ 窗格菜单
// 无 Q_OBJECT：QToolButton + lambda 连接（ADR-014 触发式 moc 结论）。
// 数据：term_session_url / term_stats / sess_active_config（状态在 Rust，铁律 2）。
#pragma once

#include <QWidget>

#include <cstdint>
#include <functional>

class QLabel;
class QToolButton;
class QLineEdit;
class QStackedWidget;

class SessionBanner : public QWidget {
public:
    /// Page menu callback is supplied by MainWindow.
    using Action = std::function<void()>;

    explicit SessionBanner(QWidget *parent = nullptr);

    void bind(uint32_t session_id);
    void edit_address();
    /// 自检辅助：面包屑文本（完整地址 URL，不省略）。
    QString breadcrumb() const;
    /// 自检辅助：窗格头几何文本（不得与 QWidget::geometry() 同名）。
    QString grid_text() const;

    /// 页头左簇：常用动作图标条（由 MainWindow 装配；插在 ⓘ 之前并附 1px 竖线）。
    void set_lead_widget(QWidget *lead);

    /// ⋮ 窗格菜单；▶ 发送；⌄ 地址下拉；> 直接进入地址编辑。
    void set_actions(Action pane_menu, Action send_action, Action recent_action);

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;
    /// 窄分组中保留地址编辑空间。
    void resizeEvent(QResizeEvent *e) override;

private:
    /// 按当前会话重建面包屑分段（协议 ▸ 分组路径 ▸ 会话名；段=可点按钮）。
    void rebuild_crumb();
    void connect_address();
    /// 22px 图标位按钮：矢量图标走 icon_cache（物理像素对齐 + 进程级缓存）。
    QToolButton *make_icon_button(const QString &name, const QString &tip);

    uint32_t session_ = 0;
    QString address_text_;         // 完整地址 URL（breadcrumb() 返回它）
    QLabel *geom_ = nullptr;       // 80×24 · UTF-8（不入布局；自检读取 grid_text()）
    QWidget *lead_ = nullptr;      // 左簇图标条（set_lead_widget 注入）
    QWidget *crumb_ = nullptr;     // 面包屑分段容器（rebuild_crumb 重建）
    QStackedWidget *address_stack_ = nullptr;
    QLineEdit *address_ = nullptr;
    bool connecting_ = false;
    QToolButton *info_btn_ = nullptr;
    QToolButton *send_btn_ = nullptr;
    QToolButton *go_btn_ = nullptr;
    QToolButton *recent_btn_ = nullptr;
    QToolButton *dots_btn_ = nullptr;
};
