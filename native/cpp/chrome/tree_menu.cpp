// tree_menu 实现（对齐批次 8）。菜单项文案/顺序/边界逐条照抄参考：
//   index.ts:368-375  连接/编辑/重命名/复制/移动到分组/导出/─/删除
//   index.ts:399      复制 = 无对话框直接落盘（session-config-helpers.ts:66-78）
//   index.ts:401-408  移动到分组 = 预填当前分组；trim；未变则不落盘；空 = 移出分组
//   index.ts:410-416  删除前确认，确认文案含会话名
//   index.ts:21-53    重命名 = 行内输入框（Qt 无行内编辑委托 ⇒ QInputDialog，见 §6 适配）
//   zh-CN.ts:208-220  文案与「副本」「movePrompt」「importEmpty/importError」
#include "cpp/chrome/tree_menu.h"

#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/session_form.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QCursor>
#include <QDate>
#include <QFileDialog>
#include <QInputDialog>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>

namespace {

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 参考导出文件名口径（`tideterm-sessions-<YYYY-MM-DD>.json`）+ 会话名单数。
QString suggest_export_name(const QString &sess_name) {
    return QStringLiteral("tideterm-session-%1-%2.json")
        .arg(sess_name.isEmpty() ? QStringLiteral("untitled") : sess_name,
             QDate::currentDate().toString(QStringLiteral("yyyy-MM-dd")));
}

} // namespace

namespace chrome {

QString tree_action(MainWindow *win, const QString &id, const QString &key,
                    const QString &arg) {
    if (win == nullptr || (id.isEmpty() && key != QLatin1String("import"))) {
        return {}; // import 是唯一无会话目标的操作（树空白区右键）
    }
    if (key == QLatin1String("connect")) {
        open_saved_session(win, id);
        return {};
    }
    if (key == QLatin1String("rename")) {
        const QString name = arg.trimmed();
        if (name.isEmpty()) {
            return {}; // 参考：空名 = 取消，不落盘
        }
        return sess_rename(rs(id), rs(name))
                   ? i18n::tr("已重命名为 %1").arg(name)
                   : i18n::tr("重命名失败：%1").arg(id);
    }
    if (key == QLatin1String("duplicate")) {
        const rust::String new_id = sess_duplicate(rs(id));
        if (new_id.empty()) {
            return i18n::tr("复制失败：%1").arg(id);
        }
        win->refresh_chrome();
        return i18n::tr("已复制为 %1").arg(q(new_id));
    }
    if (key == QLatin1String("move")) {
        SessCfg cfg = sess_form_get(rs(id), 0);
        const QString next = arg.trimmed();
        if (next == q(cfg.group_path)) {
            return {}; // 参考：与当前分组相同 = 无事发生
        }
        cfg.group_path = rs(next);
        const rust::String saved = sess_save(cfg);
        win->refresh_chrome();
        return saved.empty() ? i18n::tr("移动分组失败：%1").arg(id)
                             : i18n::tr("已移动到分组 %1").arg(
                                   next.isEmpty() ? i18n::tr("（未分组）") : next);
    }
    if (key == QLatin1String("favorite")) {
        const SessCfg cur = sess_form_get(rs(id), 0);
        sess_set_favorite(rs(id), !cur.favorite);
        win->refresh_chrome();
        return cur.favorite ? i18n::tr("已取消收藏 %1").arg(id)
                            : i18n::tr("已收藏 %1").arg(id);
    }
    if (key == QLatin1String("export")) {
        if (arg.isEmpty()) {
            return {}; // 取消文件对话框
        }
        const QString err = q(sess_export(rs(id), rs(arg)));
        return err.isEmpty() ? i18n::tr("已导出 %1").arg(arg) : err;
    }
    if (key == QLatin1String("import")) {
        if (arg.isEmpty()) {
            return {};
        }
        const qint64 n = sess_import(rs(arg));
        win->refresh_chrome();
        if (n < 0) {
            return i18n::tr("导入失败：文件格式错误或内容无效");
        }
        if (n == 0) {
            return i18n::tr("未找到有效的会话配置");
        }
        return i18n::tr("已导入 %1 个会话").arg(n);
    }
    if (key == QLatin1String("delete")) {
        if (sess_delete(rs(id))) {
            win->refresh_chrome();
            return i18n::tr("已删除会话 %1").arg(id);
        }
        return i18n::tr("会话不存在：%1").arg(id);
    }
    return {};
}

void show_tree_menu(MainWindow *win, const QModelIndex &idx) {
    const QString live_id = idx.data(PodTreeModel::RoleNodeId).toString();
    if (live_id.startsWith(QStringLiteral("live:"))) {
        bool valid = false;
        const auto id = live_id.mid(5).toUInt(&valid);
        if (valid) show_view_menu(win, id);
        return;
    }
    if (win == nullptr || !idx.isValid() || idx.data(PodTreeModel::RoleNodeId).isNull()) {
        return; // 空白区由调用方走导入（参考 index.ts:129-135）
    }
    const QString id = idx.data(PodTreeModel::RoleNodeId).toString();
    if (id.startsWith(QStringLiteral("group:"))) {
        return; // 参考：分组行不弹菜单
    }
    const SessCfg cfg = sess_form_get(rs(id), 0);
    QMenu menu(win);
    chrome::style_menu(&menu); // WT 对齐批次 4：菜单观感（行高 27 + 图标槽）
    // 参考（webview 版 index.ts:368-375）的项序保留；补图标（WT 右键菜单统一带图标）。
    QAction *connect = chrome::add_action(&menu, QStringLiteral("link"),
        i18n::tr("连接"), true, nullptr);
    QAction *edit = chrome::add_action(&menu, QStringLiteral("settings"),
        i18n::tr("编辑"), true, nullptr);
    QAction *rename = chrome::add_action(&menu, QStringLiteral("rename"),
        i18n::tr("重命名"), true, nullptr);
    QAction *duplicate = chrome::add_action(&menu, QStringLiteral("duplicate"),
        i18n::tr("复制"), true, nullptr);
    QAction *move = chrome::add_action(&menu, QStringLiteral("move"),
        i18n::tr("移动到分组"), true, nullptr);
    QAction *export_a = chrome::add_action(&menu, QStringLiteral("download"),
        i18n::tr("导出"), true, nullptr);
    QAction *fav = chrome::add_action(&menu, QStringLiteral("star"),
        cfg.favorite ? i18n::tr("取消收藏") : i18n::tr("收藏"), true, nullptr);
    chrome::add_sep(&menu);
    QAction *del = chrome::add_action(&menu, QStringLiteral("delete"),
        i18n::tr("删除"), true, nullptr);
    QAction *picked = menu.exec(QCursor::pos());
    if (picked == nullptr) {
        return;
    }
    QString msg;
    if (picked == edit) {
        SessCfg copy = cfg;
        bool connect_now = false;
        if (run_session_form(win, &copy, &connect_now)) {
            if (connect_now) {
                open_saved_session(win, q(copy.id));
            }
        }
        win->refresh_chrome();
        return;
    }
    if (picked == rename) {
        bool ok = false;
        const QString name = QInputDialog::getText(win, i18n::tr("重命名"),
            i18n::tr("会话名称"), QLineEdit::Normal, q(cfg.name), &ok);
        if (!ok) {
            return;
        }
        msg = tree_action(win, id, QStringLiteral("rename"), name);
    } else if (picked == move) {
        bool ok = false;
        const QString group = QInputDialog::getText(win, i18n::tr("移动到分组"),
            i18n::tr("新的分组路径（用 > 分隔，最多 3 级）："), QLineEdit::Normal,
            q(cfg.group_path), &ok);
        if (!ok) {
            return;
        }
        msg = tree_action(win, id, QStringLiteral("move"), group);
    } else if (picked == export_a) {
        const QString path = QFileDialog::getSaveFileName(win, i18n::tr("导出会话"),
            suggest_export_name(q(cfg.name)), i18n::tr("会话配置 (*.json)"));
        msg = tree_action(win, id, QStringLiteral("export"), path);
    } else if (picked == del) {
        if (QMessageBox::question(win, i18n::tr("确认删除会话"),
                QStringLiteral("%1？").arg(q(cfg.name))) != QMessageBox::Yes) {
            return;
        }
        msg = tree_action(win, id, QStringLiteral("delete"));
    } else if (picked == duplicate) {
        msg = tree_action(win, id, QStringLiteral("duplicate"));
    } else if (picked == connect) {
        msg = tree_action(win, id, QStringLiteral("connect"));
    } else if (picked == fav) {
        msg = tree_action(win, id, QStringLiteral("favorite"));
    }
    if (!msg.isEmpty()) {
        win->note(msg);
    }
}

void tree_import_by_prompt(MainWindow *win) {
    const QString path = QFileDialog::getOpenFileName(win, i18n::tr("导入会话"),
        QString(), i18n::tr("会话配置 (*.json)"));
    const QString msg = tree_action(win, QString(), QStringLiteral("import"), path);
    if (!msg.isEmpty()) {
        win->note(msg);
    }
}

/// 会话**视图**右键菜单（WT 对齐批次 4）：项序照参考截图 5。参考那份菜单是「视图/标签」
/// 级操作（关闭视图、拆分视图、复制会话 URL…），所以落在「会话」dock 的活跃会话列上，
/// 而不是资源管理器的已保存会话树（后者是 webview 基准的配置级菜单，见 show_tree_menu）。
/// 动作一律代理菜单栏里同 tid 的 QAction ⇒ 行为仍走 dispatch 单点。
void show_view_menu(MainWindow *win, uint32_t tab_id) {
    if (win == nullptr || tab_id == UINT32_MAX) {
        return;
    }
    win->focus_session(tab_id); // 先切到被右键的会话，代理动作才作用于它
    QMenu menu(win);
    chrome::style_menu(&menu);
    QMenuBar *bar = win->menuBar();
    auto src = [bar](const char *tid) { return find_menu_action(bar, QLatin1String(tid)); };
    auto item = [&](const char *tid, const char *icon, const QString &label,
                    const QString &keys = QString()) {
        const QString text =
            keys.isEmpty() ? label : label + QLatin1Char('\t') + keys;
        return proxy_action(&menu, tid == nullptr ? nullptr : src(tid), QLatin1String(icon),
                            text);
    };
    item(nullptr, "palette", i18n::tr("设置标签页颜色(C)")); // 引擎无标签色 ⇒ 置灰
    item("tools:sync-input", "sync", i18n::tr("同步输入…"));
    item(nullptr, "duplicate", i18n::tr("重复视图(D)")); // 无「视图复制」能力 ⇒ 置灰
    item(nullptr, "rename", i18n::tr("重命名视图(R)"));   // 无行内改名 ⇒ 置灰
    item("edit:copy-session-name", "copy", i18n::tr("复制会话名称(N)"));
    item("edit:copy-session-url", "copy", i18n::tr("复制会话 URL(U)"));
    add_sep(&menu);
    item("session:reconnect", "refresh", i18n::tr("重新连接会话(R)"));
    item("session:save", "save", i18n::tr("保存会话(S)"), QStringLiteral("Ctrl+Shift+S"));
    add_sep(&menu);
    item("window:split-view-horizontal", "split_h", i18n::tr("水平拆分视图(H)"),
         QStringLiteral("Alt+W, Alt+H"));
    item("window:split-view-vertical", "split_v", i18n::tr("垂直拆分视图(V)"),
         QStringLiteral("Alt+W, Alt+V"));
    item(nullptr, "move", i18n::tr("拆分为(S)"));    // 无分组拆分 ⇒ 置灰
    item(nullptr, "move", i18n::tr("移动至分组(M)")); // 视图级无此项 ⇒ 置灰
    add_sep(&menu);
    item("window:close-pane", "close", i18n::tr("关闭视图(C)"), QStringLiteral("Ctrl+Shift+W"));
    item("session:close-all-views", "close", i18n::tr("关闭所有视图(A)"));
    item(nullptr, "close", i18n::tr("关闭所有非活动视图(A)"));
    item(nullptr, "close", i18n::tr("关闭右侧所有视图(R)"));
    item(nullptr, "reopen", i18n::tr("重新打开已关闭视图(R)"), QStringLiteral("Ctrl+Shift+T"));
    add_sep(&menu);
    item(nullptr, "settings", i18n::tr("会话设置…(S)")); // 视图→配置 id 无映射 ⇒ 置灰
    menu.exec(QCursor::pos());
}

} // namespace chrome
