#include "cpp/clipboard.h"
// menu_bar 实现：canonical 菜单表（menu_model.rs 镜像）+ 一元化分发。

#include <QDesktopServices>
#include <QFileInfo>
#include <QUrl>
#include <iterator>

#include "cpp/chrome/ai_panel.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/menu_bar.h"
#include "cpp/chrome/command_palette.h"
#include "cpp/chrome/multiline_dialog.h"
#include "cpp/chrome/menu_items.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/i18n.h"
#include "cpp/chrome/menu_actions.h"
#include "cpp/chrome/log_actions.h"
#include "cpp/chrome/prefs_dialog.h"
#include "cpp/chrome/quick_connect.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_vault.h"

#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/chrome/window_frame.h"
#include "cpp/terminal/terminal_view.h"

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QDockWidget>
#include <QInputDialog>
#include <QKeySequence>
#include <QMenuBar>
#include <QMenu>
#include <QStatusBar>
#include <iterator>

namespace {

using namespace chrome::menu_data;

constexpr Item kLog[] = {
    {"session:log-start", "开始日志", "", kNone, nullptr, 0},
    {"session:log-stop", "停止日志", "", kNone, nullptr, 0},
    {"session:log-pause", "暂停日志", "", kNone, nullptr, 0},
    {"session:log-resume", "恢复日志", "", kNone, nullptr, 0},
    {"session:log-open", "打开日志目录", "", kNone, nullptr, 0},
    {"session:log-settings", "日志设置…", "", kNone, nullptr, 0},
};
constexpr Item kPrefs[] = {
    {"pref:general", "常规", "", kNone, nullptr, 0},
    {"pref:terminal", "终端", "", kNone, nullptr, 0},
    {"pref:ssh", "SSH", "", kNone, nullptr, 0},
};
constexpr Item kBars[] = {
    {"view:toggle-menu-bar", "菜单栏", "", kCheck | kOn, nullptr, 0},
    {"view:toggle-status-bar", "状态栏", "", kCheck | kOn, nullptr, 0},
};
constexpr Item kPanes[] = {
    {"view:toggle-left-panel", "左侧面板", "", kCheck | kOn, nullptr, 0},
    {"view:toggle-right-panel", "右侧面板", "", kCheck | kOn, nullptr, 0},
    // 「底部窗格」随底部 AI 面板一并去掉：该项原先只用于开关 AI dock，
    // 面板改为浮窗后已无对应的底部面板（2026-09-26）。
};

// Return stays with terminal input; reconnect is an explicit action.
constexpr Item kSession[] = {
    {"session:new", "新建会话", "Alt+N", kNone, nullptr, 0},
    {"session:new-window", "新建窗口", "Ctrl+Shift+N", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"session:disconnect", "断开会话", "", kNone, nullptr, 0},
    {"session:reconnect", "重新连接会话", "", kNone, nullptr, 0},
    {"session:open", "打开会话…", "", kNone, nullptr, 0},
    {"session:recent", "最近打开", "", kNone, nullptr, 0},
    {"session:reopen-closed", "重新打开已关闭标签", "Ctrl+Shift+T", kNone, nullptr, 0},
    {"session:workspace-save", "保存会话工作区", "", kNone, nullptr, 0},
    {"session:workspace-open", "恢复会话工作区…", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"session:save", "保存会话", "Ctrl+Shift+S", kNone, nullptr, 0},
    {"session:save-selection", "将选择另存为…", "", kNone, nullptr, 0},
    {"session:log", "日志", "", kSub, kLog, int(std::size(kLog))},
    {"session:preferences", "首选项", "", kSub, kPrefs, 3},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"session:close-view", "关闭视图", "Ctrl+Shift+W", kNone, nullptr, 0},
    {"session:close-all-views", "关闭所有视图", "", kNone, nullptr, 0},
    {"session:close-window", "关闭窗口", "Ctrl+Shift+Q", kNone, nullptr, 0},
    {"session:exit", "退出", "Alt+F4", kNone, nullptr, 0},
};
constexpr Item kSearch[] = {
    {"search:find", "查找…", "Ctrl+Shift+F", kNone, nullptr, 0},
    {"search:online", "在线搜索", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"search:find-next", "查找下一个", "Alt+F3", kNone, nullptr, 0},
    {"search:find-previous", "查找上一个", "Alt+Shift+F3", kNone, nullptr, 0},
    {"search:find-next-selection", "查找下一个选择", "Ctrl+Alt+F3", kNone, nullptr, 0},
    {"search:find-previous-selection", "查找上一个选择", "Ctrl+Shift+F3", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"search:highlight", "高亮文本…", "", kNone, nullptr, 0},
};
constexpr Item kSelect[] = {
    {"select:select", "选择", "", kNone, nullptr, 0},
    {"select:preview", "预览选择…", "", kNone, nullptr, 0},
    {"select:multiple", "追加选择模式", "", kCheck, nullptr, 0},
    {"select:path-components", "双击按路径分隔符选择", "", kCheck, nullptr, 0},
    {"select:select-to", "选择到", "", kNone, nullptr, 0},
    {"select:select-column-to", "选择列到", "", kNone, nullptr, 0},
    {"select:activate-next", "激活下一个选择", "", kNone, nullptr, 0},
    {"select:activate-previous", "激活上一个选择", "", kNone, nullptr, 0},
    {"select:lock", "锁定选择", "", kCheck, nullptr, 0},
    {"select:delete", "删除选择", "", kNone, nullptr, 0},
    {"select:deselect", "取消选择", "", kNone, nullptr, 0},
    {"select:split", "拆分选择", "", kNone, nullptr, 0},
    {"select:invert", "反转选择", "", kNone, nullptr, 0},
    {"select:toggle-stream-column", "列选择模式", "", kCheck, nullptr, 0},
    {"select:swap-anchor", "交换插入符和锚点", "", kNone, nullptr, 0},
};
constexpr Item kGoto[] = {
    {"goto:address-bar", "转到地址栏", "Alt+D", kNone, nullptr, 0},
    {"goto:session", "转到会话…", "Alt+O", kNone, nullptr, 0},
    {"goto:position", "转到位置…", "Alt+G", kNone, nullptr, 0},
    {"goto:symbol", "转到符号…", "Alt+`", kNone, nullptr, 0},
    {"goto:command-outline", "命令大纲…", "", kNone, nullptr, 0},
    {"goto:previous-command", "转到上一个命令(P)", "Ctrl+Shift+Left", kNone, nullptr, 0},
    {"goto:next-command", "转到下一个命令(N)", "Ctrl+Shift+Right", kNone, nullptr, 0},
};
constexpr Item kView[] = {
    {"view:any-panel", "任意面板…", "Alt+S", kNone, nullptr, 0},
    {"view:command-palette", "命令面板…", "Alt+;", kNone, nullptr, 0},
    {"view:theme", "主题…", "", kNone, nullptr, 0},
    {"view:mux-palette", "Mux/Tmux Palette", "Alt+B", kDis, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"view:hex", "Hex 进制", "", kDis, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"view:bars", "栏", "", kSub, kBars, 2},
    {"view:panes", "窗格", "", kSub, kPanes, int(std::size(kPanes))},
    {"view:syntax", "语法方案", "", kDis, nullptr, 0},
    {"view:highlight-caret-line", "高亮插入符所在行", "", kCheck | kOn, nullptr, 0},
    {"view:margin", "边距", "", kDis, nullptr, 0},
    {"view:symbols", "符号", "", kDis, nullptr, 0},
    {"view:whitespace", "空格", "", kDis, nullptr, 0},
    {"view:fold", "折叠/展开当前位置", "", kNone, nullptr, 0},
    {"view:fold-selection", "折叠所选输出", "", kNone, nullptr, 0},
    {"view:fold-regions", "折叠区域…", "", kNone, nullptr, 0},
    {"view:zoom", "缩放", "", kNone, nullptr, 0},
    {"view:opacity", "窗口不透明度", "", kDis, nullptr, 0},
};
constexpr Item kLockScreen[] = {
    {"mode:lock-screen", "立即锁屏", "Ctrl+Alt+L", kNone, nullptr, 0},
    {"mode:lock-settings", "锁屏设置…", "", kNone, nullptr, 0},
};
constexpr Item kMode[] = {
    {"mode:remote", "远程模式", "", kCheck | kOn, nullptr, 0},
    {"mode:local", "本地模式", "Ctrl+Enter", kCheck, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"mode:hex-edit", "Hex 编辑", "", kCheck, nullptr, 0},
    {"mode:word-wrap", "自动换行", "", kCheck | kDis, nullptr, 0},
    {"mode:virtual-space", "虚拟空间", "", kCheck | kDis, nullptr, 0},
    {"mode:multi-caret", "多个插入符号", "", kCheck | kDis, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"mode:focus", "专注模式", "Alt+Enter", kNone, nullptr, 0},
    {"mode:focus-settings", "专注模式设置…", "", kNone, nullptr, 0},
    {"mode:fullscreen", "全屏", "", kCheck, nullptr, 0},
    {"mode:always-on-top", "总是置顶", "", kCheck, nullptr, 0},
    {"mode:lock-menu", "锁屏", "", kSub, kLockScreen, 2},
    {"mode:debug", "调试模式", "", kDis, nullptr, 0},
};
constexpr Item kTools[] = {
    {"tools:sender", "新建发送窗口…", "", kNone, nullptr, 0},
    {"tools:sync-input", "同步输入…", "Ctrl+Shift+M", kNone, nullptr, 0},
    {"tools:onekeys", "OneKeys…", "", kNone, nullptr, 0},
    {"tools:triggers", "触发器…", "", kDis, nullptr, 0},
    {"tools:tunnels", "隧道…", "", kDis, nullptr, 0},
    {"tools:transfer-binary", "X/Y/Z Modem…", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"panel:sftp", "SFTP 浏览器", "", kNone, nullptr, 0},
    {"panel:quick", "快速连接", "", kNone, nullptr, 0},
    {"panel:log", "日志面板", "", kNone, nullptr, 0},
};
constexpr Item kAi[] = {
    {"ai:natural-language", "自然语言命令", "Ctrl+Shift+Space", kNone, nullptr, 0},
    {"ai:explain-error", "解释错误", "Ctrl+E", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"panel:ai", "AI 助手", "", kNone, nullptr, 0},
    {"ai:settings", "AI 设置…", "", kNone, nullptr, 0},
};
constexpr Item kWindow[] = {
    {"window:split-view-horizontal", "水平分割视图", "Alt+W, Alt+H", kNone, nullptr, 0},
    {"window:split-view-vertical", "垂直分割视图", "Alt+W, Alt+V", kNone, nullptr, 0},
    {"window:split-terminal-horizontal", "水平拆分终端区域", "", kNone, nullptr, 0},
    {"window:split-terminal-vertical", "垂直拆分终端区域", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"window:split-group-horizontal", "水平分割组", "Ctrl+Alt+H", kNone, nullptr, 0},
    {"window:split-group-vertical", "垂直分割组", "Ctrl+Alt+V", kNone, nullptr, 0},
    {"window:merge-to-group", "合并到组", "", kNone, nullptr, 0},
    {"window:close-group", "关闭组", "", kNone, nullptr, 0},
    {"window:close-pane", "关闭窗格", "Ctrl+Shift+W", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"window:zoom-pane", "放大/还原窗格", "Alt+Z", kNone, nullptr, 0},
    {"window:activate-next", "激活下一个", "Alt+[", kNone, nullptr, 0},
    {"window:activate-previous", "激活上一个", "Alt+]", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"window:manager", "窗口管理器", "", kDis, nullptr, 0},
    {"window:windows-list", "窗口列表", "", kDis, nullptr, 0},
};
constexpr Item kHelp[] = {
    {"help:shortcuts", "快捷键", "Ctrl+/", kNone, nullptr, 0},
    {"help:documentation", "文档", "", kNone, nullptr, 0},
    {"help:release-notes", "发布说明", "", kNone, nullptr, 0},
    {"help:report-issue", "报告问题", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"help:about", "关于", "", kNone, nullptr, 0},
};

struct TopMenu {
    const char *label;
    const Item *items;
    int n;
};

constexpr TopMenu kMenus[] = {
    {"会话", kSession, int(std::size(kSession))}, {"编辑", kEdit, int(std::size(kEdit))}, {"搜索", kSearch, 9},
    {"选择", kSelect, int(std::size(kSelect))}, {"转到", kGoto, int(std::size(kGoto))}, {"查看", kView, int(std::size(kView))},
    {"模式", kMode, 14},    {"工具", kTools, 9},   {"AI", kAi, 5},
    {"窗口", kWindow, 15},  {"帮助", kHelp, 6},
};

namespace {

QString mq(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String mrs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 「新建会话」：表单新建（保存 + 可选立即连接）。空白预填 local 页。
void act_new_session(MainWindow *win) {
    SessCfg cfg = sess_form_get(mrs(QString()), 0);
    bool connect_now = false;
    if (run_session_form(win, &cfg, &connect_now)) {
        win->note(i18n::tr("会话已保存：%1").arg(mq(cfg.name)));
        if (connect_now) {
            const uint32_t tab = chrome::sess_open_with_vault(win, cfg.id);
            if (tab != UINT32_MAX && tab != chrome::kVaultLocked) {
                win->add_tab_with(tab);
                chrome::record_recent(mq(cfg.id));
            }
        }
        win->refresh_chrome();
    }
}

/// 「打开会话…」：已保存会话列表选择 → sess_open。

} // namespace

/// 一元化分发：全部菜单动作的唯一切换点（对照 Slint 壳双分发铁律）。
void dispatch(MainWindow *win, QAction *a) {
    const QByteArray id = a->property("tid").toByteArray();
    const bool on = a->isChecked();
    PtyView *v = win->active_view();
    if (chrome::dispatch_log(win, a)) return;
    if (id == "completion:settings" || id == "completion:refresh") { win->show_completion_sources(id == "completion:refresh"); return; }

    if (id == "session:reopen-closed") { win->reopen_closed_tab(); return; }
    if (id == "session:workspace-save") { win->save_workspace(); return; }
    if (id == "session:workspace-open") { win->choose_workspace(); return; }
    if (id == "session:new") {
        act_new_session(win);
        return;
    } else if (id == "session:open" || id == "goto:session") {
        chrome::show_command_palette(win, QStringLiteral("tm "));
        return;
    } else if (id == "session:disconnect") {
        if (v != nullptr && term_disconnect_tab(v->tab())) {
            win->note(i18n::tr("已断开当前会话"));
        }
        win->refresh_chrome();
        return;
    } else if (id == "session:reconnect") {
        if (v != nullptr && term_reconnect_tab(v->tab())) {
            win->note(i18n::tr("正在重新连接当前会话"));
        }
        win->refresh_chrome();
        return;
    } else if (id == "ai:natural-language") {
        if (AiPanel *ai = ensure_ai_panel(win)) {
            win->docks()->bottom->show();
            ai->ask_command(QString());
            ai->status()->setText(
                i18n::tr("输入自然语言描述后回车"));
        }
        return;
    } else if (id == "ai:explain-error") {
        if (AiPanel *ai = ensure_ai_panel(win)) {
            win->docks()->bottom->show();
            ai->ask_explain(QString());
            ai->status()->setText(i18n::tr("粘贴报错文本后回车"));
        }
        return;
    } else if (id == "panel:ai") {
        if (win->docks() != nullptr && win->docks()->bottom != nullptr) {
            win->docks()->bottom->setVisible(!win->docks()->bottom->isVisible());
        }
        return;
    } else if (id == "ai:settings") {
        if (run_ai_settings(win)) {
            win->note(i18n::tr("AI 设置已保存（key 仅存内存）"));
        }
        return;
    } else if (id == "pref:general" || id == "pref:terminal" || id == "pref:ssh") {
        // 首选项三入口（⑦）：同一对话框定位对应页；保存后已应用主题。
        const int page = id == "pref:general" ? 0 : (id == "pref:terminal" ? 1 : 2);
        if (run_prefs_dialog(win, page)) {
            win->note(i18n::tr("首选项已保存"));
        }
        return;
    } else if (id == "panel:sftp") {
        if (win->docks() != nullptr && win->docks()->files != nullptr) {
            win->docks()->files->setVisible(!win->docks()->files->isVisible());
        }
        return;
    } else if (id == "panel:quick") {
        chrome::open_quick_connect(win);
        return;
    } else if (id == "goto:address-bar") {
        chrome::focus_quick_connect(win);
        return;
    } else if (id == "session:new-window") {
        auto *w2 = new MainWindow();
        w2->setAttribute(Qt::WA_DeleteOnClose);
        w2->show();
    } else if (id == "session:close-view") {
        win->close_current_tab();
    } else if (id == "session:exit") {
        win->close();
    } else if (id == "edit:copy") {
        if (v != nullptr) {
            const SelectionText selected = term_selection_text(v->tab());
            if (selected.exceeds_budget) {
                win->note(i18n::tr("选择内容超过 1 MiB，请缩小范围后复制")); return;
            }
            const rust::String &t = selected.text;
            const QString q = QString::fromUtf8(t.data(), int(t.size()));
            if (!q.isEmpty()) {
                if (host_clipboard::copy_text(q)) win->note(i18n::tr("已复制 %1 字符").arg(q.size()));
                else win->note(i18n::tr("剪贴板为空或不可用"));
            } else {
                win->note(i18n::tr("没有已选内容可复制"));
            }
        }
        return; // 有明确反馈，不走未实现兜底
    } else if (id == "edit:paste") {
        if (v != nullptr) chrome::paste_clipboard(v);
        if (v) v->sync_reading_mode();
        win->refresh_chrome();
        return;
    } else if (id == "view:toggle-menu-bar") {
        chrome::set_main_menus_visible(win, on);
        return;
    } else if (id == "view:toggle-status-bar") {
        win->statusBar()->setVisible(on);
        return;
    } else if (id == "view:toggle-left-panel") {
        if (win->docks() != nullptr && win->docks()->conn != nullptr) {
            win->docks()->conn->setVisible(on);
        }
        return;
    } else if (id == "view:toggle-right-panel") {
        if (win->docks() != nullptr && win->docks()->sess != nullptr) {
            win->docks()->sess->setVisible(on);
        }
        return;
    } else if (id == "window:split-view-horizontal") {
        win->split_group(Qt::Horizontal, true);
    } else if (id == "window:split-view-vertical") {
        win->split_group(Qt::Vertical, true);
    } else if (id == "window:split-terminal-horizontal") {
        win->split_active(Qt::Horizontal);
    } else if (id == "window:split-terminal-vertical") {
        win->split_active(Qt::Vertical);
    } else if (id == "window:close-pane") {
        if (v != nullptr) {
            win->close_pane(v);
        }
    } else if (id == "window:zoom-pane") {
        win->toggle_zoom_active();
    } else if (id == "window:activate-next") {
        win->focus_pane_delta(1);
    } else if (id == "window:activate-previous") {
        win->focus_pane_delta(-1);
    } else if (id == "help:about") {
        win->note(i18n::tr("TideTerm Qt 壳（Phase C ③ chrome 骨架）· 引擎 "
                                 "tiderm-terminal · 视图 QtWidgets"));
        return;
    } else if (chrome::dispatch_extra(win, a)) {
        return;
    } else {
        win->note(i18n::tr("未实现：%1").arg(a->text()));
        return;
    }
    win->refresh_chrome();
}

/// 递归装配一组条目到菜单。
void add_items(MainWindow *win, QMenu *menu, const Item *items, int n) {
    for (int i = 0; i < n; ++i) {
        const Item &it = items[i];
        if (it.flags & kSep) {
            menu->addSeparator();
            continue;
        }
        if (it.flags & kSub) {
            QMenu *sub = menu->addMenu(i18n::tr(it.label));
            chrome::set_action_icon(sub->menuAction(), QString::fromLatin1(it.id));
            add_items(win, sub, it.children, it.n_children);
            continue;
        }
        QAction *a = menu->addAction(i18n::tr(it.label));
        a->setProperty("tid", QByteArray(it.id));
        if (it.shortcut != nullptr && it.shortcut[0] != '\0') {
            a->setShortcut(QKeySequence(QString::fromUtf8(it.shortcut)));
        }
        if (it.flags & kCheck) {
            a->setCheckable(true);
            a->setChecked((it.flags & kOn) != 0);
        }
        chrome::set_action_icon(a, QString::fromLatin1(it.id));
        if (QByteArray(it.id) == "mode:local") a->setAutoRepeat(false);
        if (it.flags & kDis) {
            a->setEnabled(false);
        }
        QAction *act = a;
        QObject::connect(act, &QAction::triggered, win, [win, act] { dispatch(win, act); });
    }
}

} // namespace

#include "cpp/chrome/mode_actions.h"
namespace chrome {

void build_menu(MainWindow *win) {
    QMenuBar *bar = win->menuBar();
    bar->setObjectName(QStringLiteral("mainMenuBar"));
    for (const TopMenu &m : kMenus) {
        QMenu *menu = bar->addMenu(i18n::tr(m.label));
        add_items(win, menu, m.items, m.n);
    }
    bind_input_modes(win);
}

} // namespace chrome
