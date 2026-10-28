// term_menu 实现（WT 对齐批次 4）。见 term_menu.h 顶部：项序照参考、缺后端置灰、
// 动作走 proxy_action 代理菜单栏同 tid 的 QAction（dispatch 单点）。
#include "cpp/chrome/term_menu.h"
#include "cpp/chrome/history_tools.h"

#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/main_window.h"
#include "cpp/clipboard.h"
#include "cpp/terminal/terminal_view.h"

#include <QAction>
#include <QIcon>
#include <QMenu>
#include <QMenuBar>
#include <QPointer>

namespace chrome {

namespace {

/// 快捷键是**显示文本**（拼在 `\t` 后由 style 右对齐），不绑 QAction::setShortcut——
/// 菜单栏已有同键动作，重复绑定会让 Qt 报 ambiguous shortcut 并让两边都不响。
QString with_key(const QString &label, const QString &keys) {
    return keys.isEmpty() ? label : label + QLatin1Char('\t') + keys;
}

} // namespace

QMenu *build_term_menu(QWidget *context, uint64_t command) {
    auto *menu = new QMenu(context);
    style_menu(menu);
    auto *win = context == nullptr ? nullptr : dynamic_cast<MainWindow *>(context->window());
    QMenuBar *bar = win == nullptr ? nullptr : win->menuBar();
    auto src = [bar](const char *tid) { return find_menu_action(bar, QLatin1String(tid)); };
    auto item = [&](const char *tid, const char *icon, const QString &label,
                    const QString &keys = QString()) {
        return proxy_action(menu, tid == nullptr ? nullptr : src(tid), QLatin1String(icon),
                            with_key(label, keys));
    };
    QPointer<PtyView> view(dynamic_cast<PtyView *>(context));
    const auto tab = view ? view->tab() : UINT32_MAX;
    const bool has_command = view && command && term_command_text(tab, command).available;
    auto command_action = [&](const QString &label, bool select) {
        auto *action = add_action(menu, QStringLiteral("command"), label, has_command, [view, win, tab, command, select] {
            if (!view || view->is_closed() || view->tab() != tab) return;
            if (select) {
                if (!term_command_select_block(tab, command)) { if (win) win->note(i18n::tr("命令位置已失效")); return; }
                view->sync_reading_mode(); view->update(); if (win) win->refresh_chrome();
            } else {
                const auto value = term_command_text(tab, command);
                if (value.available) host_clipboard::copy_text(QString::fromUtf8(value.text.data(), int(value.text.size())));
                else if (win) win->note(i18n::tr("命令位置已失效"));
            }
        });
        if (!has_command) action->setToolTip(i18n::tr("请在已识别的命令或其输出上右键；命令大纲可查看已保留命令"));
    };

    // 剪贴板组（WT：复制 / 复制命令 / 粘贴 / 粘贴选定的文本）
    item("edit:copy", "copy", i18n::tr("复制(C)"), QStringLiteral("Shift+Del"));
    command_action(i18n::tr("复制命令(C)"), false);
    item("edit:paste", "paste", i18n::tr("粘贴(P)"), QStringLiteral("Shift+Ins"));
    item("edit:paste-selected", "paste", i18n::tr("粘贴选定的文本(S)"),
         QStringLiteral("Ctrl+Shift+X"));
    add_sep(menu);

    // 高亮 / 查找 / 在线搜索
    item("search:highlight", "highlight", i18n::tr("高亮文本…(H)"));
    item("search:find", "search", i18n::tr("查找…(F)"), QStringLiteral("Ctrl+Shift+F"));
    item("search:online", "search", i18n::tr("在线搜索(S)"));
    item("completion:settings", "settings", i18n::tr("命令建议来源设置…"));
    item("completion:refresh", "refresh", i18n::tr("采集 Shell 名字与资源…"));
    add_sep(menu);

    // 清除组：复用 Rust pane 操作，快捷键由菜单栏统一持有。
    auto *history = menu->addAction(i18n::tr("历史导出与浏览…"));
    history->setObjectName(QStringLiteral("historyToolsAction")); history->setEnabled(view && !view->is_closed());
    QObject::connect(history, &QAction::triggered, menu, [view] { if (view) show_history_tools(view); });
    auto *tmux = menu->addAction(i18n::tr("tmux 会话…"));
    tmux->setObjectName(QStringLiteral("tmuxToolsAction")); tmux->setEnabled(view && !view->is_closed());
    QObject::connect(tmux, &QAction::triggered, menu, [view] { if (view) show_tmux_tools(view); });
    item("edit:clear-scrollback", "clear_scroll", i18n::tr("清除回滚(B)"), QStringLiteral("Ctrl+Shift+L"));
    item("edit:clear-screen", "screen_clear", i18n::tr("清屏(S)"));
    item("edit:clear-all", "screen_clear", i18n::tr("全部清除(A)"));
    add_sep(menu);

    // 选择 / 命令导航
    item("select:select", "select_all", i18n::tr("全选(A)"), QStringLiteral("Ctrl+Shift+A"));
    item("select:preview", "search", i18n::tr("预览选择…"));
    item("select:lock", "lock", i18n::tr("锁定选择"));
    item("select:split", "select_all", i18n::tr("拆分选择"));
    item("select:invert", "select_all", i18n::tr("反转选择"));
    item("select:path-components", "select_all", i18n::tr("双击按路径分隔符选择"));
    item("view:fold-selection", "command", i18n::tr("折叠所选输出"));
    item("view:fold-regions", "command", i18n::tr("折叠区域…"));
    command_action(i18n::tr("选择命令及输出"), true);
    item("goto:previous-command", "prev", i18n::tr("转到上一个命令(P)"), QStringLiteral("Ctrl+Shift+Left"));
    item("goto:next-command", "next", i18n::tr("转到下一个命令(N)"), QStringLiteral("Ctrl+Shift+Right"));
    add_sep(menu);

    // 触发器 / 传输 / 日志▸ / 重置
    item(nullptr, "trigger", i18n::tr("会话触发器…(T)"));
    item("tools:transfer-binary", "binary", i18n::tr("传输二进制(I)"));
    QMenu *log_menu = menu->addMenu(i18n::tr("日志(L)"));
    if (log_menu != nullptr) {
        style_menu(log_menu);
        log_menu->menuAction()->setIcon(
            scalable_icon(QStringLiteral("log"), metrics::kMenuIconCanvas));
        proxy_action(log_menu, src("session:log-start"), QStringLiteral("log"),
                     i18n::tr("开始日志"));
        proxy_action(log_menu, src("session:log-stop"), QStringLiteral("log"),
                     i18n::tr("停止日志"));
        proxy_action(log_menu, src("session:log-pause"), QStringLiteral("log"), i18n::tr("暂停日志"));
        proxy_action(log_menu, src("session:log-resume"), QStringLiteral("log"), i18n::tr("恢复日志"));
        proxy_action(log_menu, src("session:log-settings"), QStringLiteral("settings"), i18n::tr("日志设置…"));
        proxy_action(log_menu, src("session:log-open"), QStringLiteral("open"),
                     i18n::tr("打开日志目录"));
    }
    item(nullptr, "reset", i18n::tr("重置(R)")); // 重置子项无实现 ⇒ 置灰
    return menu;
}

} // namespace chrome
