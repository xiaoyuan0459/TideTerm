// Selection policy and edits use the same pane across menus and pointer input.
#include "cpp/chrome/selection_actions.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/terminal_preview.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/i18n.h"
#include <QAction>
#include <QMenuBar>

namespace chrome {
bool dispatch_selection(MainWindow *win, QAction *action) {
    const QByteArray id = action->property("tid").toByteArray();
    if (!id.startsWith("select:")) return false;
    auto *v = win->active_view();
    if (!v || v->is_closed() || win->property("screenLocked").toBool()) return true;
    if (id == "select:lock") {
        term_set_selection_locked(v->tab(), action->isChecked());
        win->note(term_selection_locked(v->tab()) ? i18n::tr("选择已锁定，请先解锁或取消选择") : i18n::tr("选择已解锁"));
        v->update(); win->refresh_chrome(); return true;
    }
    if (id == "select:path-components") {
        term_set_path_selection(v->tab(), action->isChecked());
        win->refresh_chrome(); return true;
    }
    if (id == "select:split" || id == "select:invert") {
        const bool invert = id == "select:invert";
        const auto error = term_selection_transform(v->tab(), invert);
        const char *message = nullptr;
        switch (error) {
        case 0: message = invert ? "已反选当前可见文本" : "已按物理行拆分活动选择"; break;
        case 1: message = "请先选择跨行文本再拆分"; break;
        case 2: message = "选择已锁定，请先解锁或取消选择"; break;
        case 3: message = "最多保留 32 个选区、处理 256K 单元格，请缩小范围"; break;
        case 4: message = "选择来源已失效，请重新选择"; break;
        default: message = "锁定屏幕期间不能操作选择"; break;
        }
        win->note(i18n::tr(message)); v->sync_reading_mode(); v->update(); win->refresh_chrome(); return true;
    }
    if (id == "select:preview") { show_terminal_preview(win, v); return true; }
    if (id == "select:multiple") {
        if (v && !v->is_closed()) {
            term_set_multi_selection(v->tab(), action->isChecked());
            win->note(i18n::tr("Ctrl 拖选追加；阅读模式 Tab 切换选区，Esc 取消全部"));
            win->refresh_chrome();
        }
        return true;
    }
    if (id == "select:activate-next" || id == "select:activate-previous" || id == "select:delete") {
        if (v && !v->is_closed()) {
            const bool changed = id == "select:delete" ? term_selection_remove_active(v->tab())
                : term_selection_cycle(v->tab(), id == "select:activate-next");
            if (changed) {
                const SelectionStatus state = term_selection_status(v->tab());
                win->note(state.count ? i18n::tr("活动选区 %1 / %2").arg(state.active).arg(state.count)
                                     : i18n::tr("已取消全部选择"));
            }
            v->update(); win->refresh_chrome();
        }
        return true;
    }
    if (id == "select:toggle-stream-column") {
        if (v && !v->is_closed()) {
            term_set_column_selection(v->tab(), action->isChecked());
            v->update(); win->refresh_chrome();
        }
        return true;
    }
    if (id == "select:select") {
        if (v) { term_select_all(v->tab()); v->update(); }
        return true;
    }
    if (id == "select:deselect") {
        if (v && !v->is_closed()) { term_deselect(v->tab()); v->update(); win->refresh_chrome(); }
        return true;
    }
    if (id == "select:select-to" || id == "select:select-column-to") {
        if (v && !v->is_closed()) {
            win->note(v->begin_select_to(id == "select:select-column-to")
                ? i18n::tr("点击选择终点；Esc 取消") : i18n::tr("请先选择文本"));
        }
        return true;
    }
    if (id == "select:swap-anchor") {
        if (v && !v->is_closed()) {
            term_selection_swap_anchor(v->tab());
            v->update();
        }
        return true;
    }
    return false;
}

void refresh_selection_actions(MainWindow *window, PtyView *view) {
    const bool readable = view && !view->is_closed() && !window->property("screenLocked").toBool();
    const auto tab = readable ? view->tab() : UINT32_MAX;
    const auto count = readable ? term_selection_status(tab).count : 0;
    const bool locked = readable && term_selection_locked(tab);
    const bool selected = readable && term_selection_active(tab);
    const auto enable = [&](const char *id, bool enabled, const QString &hint = QString()) {
        if (auto *action = find_menu_action(window->menuBar(), QLatin1String(id))) {
            action->setEnabled(enabled); action->setToolTip(hint);
        }
    };
    const auto checked = [&](const char *id, bool value) {
        if (auto *action = find_menu_action(window->menuBar(), QLatin1String(id))) action->setChecked(value);
    };
    enable("select:lock", count > 0, i18n::tr("锁定后保留选区；取消选择或来源失效会解锁"));
    checked("select:lock", locked);
    enable("select:toggle-stream-column", readable && !locked);
    checked("select:toggle-stream-column", readable && term_column_selection(tab));
    enable("select:multiple", readable && !locked);
    checked("select:multiple", readable && term_multi_selection(tab));
    enable("select:path-components", readable, i18n::tr("开启后 / 和 \\ 也作为分隔符；保存会话可保留此设置。"));
    checked("select:path-components", readable && term_path_selection(tab));
    for (const auto *id : {"select:activate-next", "select:activate-previous"}) enable(id, count > 1);
    enable("select:delete", count > 0 && !locked);
    enable("select:deselect", count > 0);
    enable("select:select", readable && !locked);
    enable("select:preview", selected);
    for (const auto *id : {"select:select-to", "select:select-column-to", "select:swap-anchor"}) enable(id, selected && !locked);
    enable("select:split", selected && !locked, i18n::tr("仅拆分活动选区；软换行也按物理行分开，复制时以换行连接"));
    enable("select:invert", readable && !locked, i18n::tr("只反选当前可见文本，不含折叠占位行和行尾空白"));
}
}
