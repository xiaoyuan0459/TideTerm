// PanelHeader —— 侧栏面板头（2026-09-25 WT 对齐批次 1）。
//
// WT 实测口径（docs/reports/qt-windterm-ui-align-2026-09-25.md §2.1）：
//   标题行 26px（左 8×8 语义色块 + 11px/600 标题 + 右侧 ⚙/✕ 工具钮）+ 可选筛选行 22px。
//
// 装配：`install_panel_header()` 用 `QDockWidget::setTitleBarWidget()` 顶掉原生标题栏。
// **dock->widget() 仍是原内容控件**（筛选行挂在标题栏 widget 内），因此 docks.cpp /
// selftest 里所有「按 dock->widget() 取内容」的代码不受影响。
#pragma once

#include <QString>
#include <QWidget>

#include <functional>
#include <memory>

class QDockWidget;
class QLabel;
class QLineEdit;
class QToolButton;

namespace chrome {
class PanelDrag;

/// 自绘面板头：标题行固定启用，筛选行按需追加。
///
/// 自绘标题栏处理拖拽：保持原布局，小标题卡跟手，预览停靠区域；释放时移动面板。
class PanelHeader : public QWidget {
public:
    explicit PanelHeader(const QString &title, QWidget *parent = nullptr);
    ~PanelHeader() override;

    void set_title(const QString &title);

    /// 标题左色块的语义色（WT 逐面板异色：资源管理器红 / 文件管理器·会话蓝 / 历史命令橙）。
    /// 取值 = `danger` / `accent` / `warn` / `ok`，落到 QSS 的 `#panelDot<Kind>` 规则，
    /// 不写内联色值（换主题才不分叉）。
    void set_dot(const QString &kind);

    /// 追加筛选行（height = kPanelFilterRowHeight），返回输入框供调用方接过滤逻辑。
    /// 重复调用只返回既有输入框（一个面板头只有一行筛选）。
    QLineEdit *add_filter(const QString &placeholder);

    QToolButton *gear_button() const { return gear_; }
    QToolButton *close_button() const { return close_; }

    /// 筛选文本变化（空串 = 清空过滤）。
    std::function<void(const QString &)> on_filter;

protected:
    bool eventFilter(QObject *object, QEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;

private:
    QLabel *dot_ = nullptr;
    QLabel *title_ = nullptr;
    QLineEdit *filter_ = nullptr;
    QToolButton *gear_ = nullptr;
    QToolButton *close_ = nullptr;
    void cancel_drag();
    void finish_drag(const QPoint &global);
    QPoint press_gp_;
    bool pressed_ = false;
    std::unique_ptr<PanelDrag> drag_;
};

/// 自绘标题栏；⚙ 接 panel_controls 的拆分/浮动/面板选择，✕ 接关闭。
/// `filter_placeholder` 非空时同时加一行筛选框（占位符即该串）。
PanelHeader *install_panel_header(QDockWidget *dock, const QString &title,
                                  const QString &filter_placeholder = QString());

} // namespace chrome
