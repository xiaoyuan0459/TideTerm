#include "cpp/clipboard.h"
#include "cpp/chrome/menu_actions.h"
#include "cpp/chrome/command_palette.h"
#include "cpp/chrome/command_outline.h"
#include "cpp/chrome/multiline_dialog.h"
#include "cpp/chrome/selection_actions.h"
#include "cpp/chrome/fold_regions.h"
#include "cpp/chrome/text_marks.h"
#include "cpp/i18n.h"
#include "cpp/chrome/modem_dialog.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/onekey_dialog.h"
#include "cpp/chrome/prefs_dialog.h"
#include "cpp/chrome/quick_connect.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/screen_lock.h"
#include "cpp/chrome/screen_lock_settings.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/session_vault.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QDesktopServices>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDockWidget>
#include <QFileDialog>
#include <QInputDialog>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QSaveFile>
#include <QSettings>
#include <QStatusBar>
#include <QUrl>
#include <QUrlQuery>
#include <QVariant>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
QSettings settings() {
    return QSettings(QSettings::defaultFormat(), QSettings::UserScope,
                     QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
}

void refresh_views(MainWindow *win) {
    for (int page = 0; page < win->page_count(); ++page) {
        for (int pane = 0; ; ++pane) {
            PtyView *view = win->pane_view(page, pane);
            if (!view) break;
            view->update();
        }
    }
}

void check_peer(MainWindow *win, const QByteArray &id, bool value) {
    for (QAction *top : win->menuBar()->actions()) {
        if (!top->menu()) continue;
        for (QAction *item : top->menu()->actions()) {
            if (item->property("tid").toByteArray() == id) item->setChecked(value);
        }
    }
}

bool save_selection(MainWindow *win, PtyView *view) {
    if (!view) return true;
    const SelectionText selected = term_selection_text(view->tab());
    if (selected.exceeds_budget) { win->note(i18n::tr("选择内容超过 1 MiB，请缩小范围后复制")); return true; }
    const QString value = q(selected.text);
    if (value.isEmpty()) { win->note(i18n::tr("当前没有选中的文本")); return true; }
    const QString path = QFileDialog::getSaveFileName(win, i18n::tr("保存选择"),
        QStringLiteral("selection.txt"), i18n::tr("文本文件 (*.txt);;所有文件 (*)"));
    if (path.isEmpty()) return true;
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly) || file.write(value.toUtf8()) < 0 || !file.commit()) {
        win->note(i18n::tr("保存失败：%1").arg(file.errorString()));
    } else { win->note(i18n::tr("已保存选择：%1").arg(path)); }
    return true;
}

bool show_recent(MainWindow *win) {
    const QStringList ids = settings().value(QStringLiteral("recentSessions")).toStringList();
    QStringList names;
    QVector<QString> found;
    for (const QString &id : ids) {
        const SessCfg cfg = sess_form_get(rs(id), 0);
        if (q(cfg.id) == id) { names << q(cfg.name); found << id; }
    }
    if (names.isEmpty()) { win->note(i18n::tr("尚无最近打开的已保存会话")); return true; }
    bool ok = false;
    const QString name = QInputDialog::getItem(win, i18n::tr("最近打开"),
        i18n::tr("会话"), names, 0, false, &ok);
    if (ok) {
        const int at = names.indexOf(name);
        if (at >= 0) {
            const uint32_t tab = chrome::sess_open_with_vault(win, rs(found[at]));
            if (tab == chrome::kVaultLocked) {
                win->note(i18n::tr("已取消：凭据库未解锁"));
            } else if (tab != UINT32_MAX) {
                win->add_tab_with(tab);
                chrome::record_recent(found[at]);
            }
        }
    }
    return true;
}


bool show_shortcuts(MainWindow *win) {
    QStringList rows;
    for (QAction *top : win->menuBar()->actions()) {
        if (!top->menu()) continue;
        for (QAction *item : top->menu()->actions()) {
            if (!item->shortcut().isEmpty()) rows << QStringLiteral("%1 › %2    %3")
                .arg(top->text(), item->text(), item->shortcut().toString());
        }
    }
    QMessageBox::information(win, i18n::tr("快捷键"), rows.join(QLatin1Char('\n')));
    return true;
}

void focus_settings(MainWindow *win) {
    QSettings s = settings();
    QDialog dlg(win); dlg.setWindowTitle(i18n::tr("专注模式设置"));
    auto *layout = new QVBoxLayout(&dlg);
    auto *hide_status = new QCheckBox(i18n::tr("隐藏状态栏"), &dlg);
    auto *hide_send = new QCheckBox(i18n::tr("隐藏发送栏"), &dlg);
    hide_status->setChecked(s.value(QStringLiteral("focusHideStatus"), true).toBool());
    hide_send->setChecked(s.value(QStringLiteral("focusHideSend"), true).toBool());
    layout->addWidget(hide_status); layout->addWidget(hide_send);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dlg);
    layout->addWidget(buttons);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dlg, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dlg, &QDialog::reject);
    if (dlg.exec() == QDialog::Accepted) {
        s.setValue(QStringLiteral("focusHideStatus"), hide_status->isChecked());
        s.setValue(QStringLiteral("focusHideSend"), hide_send->isChecked());
    }
}

void focus_toggle(MainWindow *win) {
    ChromeDocks *d = win->docks();
    if (!d) return;
    const auto panels = win->findChildren<QDockWidget *>(QString(), Qt::FindDirectChildrenOnly);
    const bool on = !win->property("focusMode").toBool();
    if (on) {
        const bool hide_send = settings().value(QStringLiteral("focusHideSend"), true).toBool();
        win->setProperty("focusHidesSend", hide_send);
        for (QDockWidget *panel : panels) {
            if (panel == d->send && !hide_send) continue;
            panel->setProperty("focusWasOpen", !panel->isHidden());
            panel->setProperty("focusWasActive", panel->isVisible());
            panel->hide();
        }
        if (settings().value(QStringLiteral("focusHideStatus"), true).toBool()) {
            win->setProperty("focusStatus", win->statusBar()->isVisible());
            win->statusBar()->hide();
        }
    } else {
        for (auto *panel : panels) if (panel->property("focusWasOpen").isValid())
            panel->setVisible(panel->property("focusWasOpen").toBool());
        for (auto *panel : panels) {
            if (panel->property("focusWasActive").toBool()) panel->raise();
            panel->setProperty("focusWasOpen", QVariant());
            panel->setProperty("focusWasActive", QVariant());
        }
        if (win->property("focusStatus").toBool()) win->statusBar()->show();
    }
    win->setProperty("focusMode", on);
}

} // namespace

namespace chrome {
void record_recent(const QString &id) {
    if (id.isEmpty()) return;
    QSettings s = settings();
    QStringList ids = s.value(QStringLiteral("recentSessions")).toStringList();
    ids.removeAll(id); ids.prepend(id);
    while (ids.size() > 10) ids.removeLast();
    s.setValue(QStringLiteral("recentSessions"), ids);
}

bool dispatch_extra(MainWindow *win, QAction *action) {
    const QByteArray id = action->property("tid").toByteArray();
    PtyView *v = win->active_view();
    if (dispatch_fold(win, action)) return true;
    if (dispatch_marks(win, action)) return true;
    if (dispatch_selection(win, action)) return true;
    if (id == "goto:command-outline") { show_command_outline(win); return true; }
    if (id == "goto:symbol" || id == "goto:position") { show_command_palette(win, id == "goto:symbol" ? QStringLiteral("@") : QStringLiteral(":")); return true; }
    if (id == "goto:previous-command" || id == "goto:next-command") {
        if (v && !v->is_closed()) {
            if (!term_command_step(v->tab(), id == "goto:next-command")) win->note(i18n::tr("没有可定位的命令"));
            v->sync_reading_mode(); v->update(); win->refresh_chrome();
        }
        return true;
    }
    if (id == "tools:transfer-binary") { run_modem_dialog(win); return true; }
    if (id == "session:recent") return show_recent(win);
    if (id == "session:save") {
        if (!v) return true;
        SessCfg cfg = sess_active_config(v->tab());
        bool connect_now = false;
        if (run_session_form(win, &cfg, &connect_now)) {
            record_recent(q(cfg.id));
            win->note(i18n::tr("会话配置已保存"));
            if (connect_now) {
                const uint32_t tab = chrome::sess_open_with_vault(win, cfg.id);
                if (tab != UINT32_MAX && tab != chrome::kVaultLocked) {
                    win->add_tab_with(tab);
                }
            }
            win->refresh_chrome();
        }
        return true;
    }
    if (id == "session:save-selection") return save_selection(win, v);
    if (id == "session:close-all-views") {
        while (win->page_count() > 1) win->close_tab(win->page_count() - 1);
        win->close_current_tab();
        return true;
    }
    if (id == "session:close-window") { win->close(); return true; }
    if (id == "edit:paste-selected") {
        if (v) {
            const rust::String selected = term_selected_text(v->tab());
            const QString text = q(selected);
            if (text.contains(QLatin1Char('\n')) || text.contains(QLatin1Char('\r')))
                show_multiline(win, v, text, 1);
            else if (!selected.empty()) term_paste(v->tab(), rust::Slice<const uint8_t>(
                reinterpret_cast<const uint8_t *>(selected.data()), selected.size()));
            v->sync_reading_mode();
            win->refresh_chrome();
        }
        return true;
    }
    if (id == "edit:copy-session-name" || id == "edit:copy-session-url") {
        if (v) host_clipboard::copy_text(q(id == "edit:copy-session-name"
            ? term_session_name(v->tab()) : term_session_url(v->tab())));
        return true;
    }
    if (id == "edit:clear-scrollback" || id == "edit:clear-screen" || id == "edit:clear-all") {
        if (v && !v->is_closed()) {
            term_clear(v->tab(), id == "edit:clear-scrollback" ? 0 : id == "edit:clear-screen" ? 1 : 2);
            v->update();
        }
        return true;
    }
    if (id == "search:find") {
        if (!v) return true;
        QDialog dialog(win);
        dialog.setObjectName(QStringLiteral("terminalSearch"));
        dialog.setWindowTitle(i18n::tr("查找终端内容"));
        auto *layout = new QVBoxLayout(&dialog);
        auto *query = new QLineEdit(q(term_search_query(v->tab())), &dialog);
        query->setObjectName(QStringLiteral("searchQuery"));
        layout->addWidget(new QLabel(i18n::tr("查找"), &dialog));
        layout->addWidget(query);
        const uint32_t flags = term_search_flags(v->tab());
        auto *match_case = new QCheckBox(i18n::tr("区分大小写"), &dialog);
        auto *regex = new QCheckBox(i18n::tr("正则表达式"), &dialog);
        auto *whole_word = new QCheckBox(i18n::tr("全词匹配"), &dialog);
        match_case->setObjectName(QStringLiteral("searchCase"));
        regex->setObjectName(QStringLiteral("searchRegex"));
        whole_word->setObjectName(QStringLiteral("searchWord"));
        match_case->setChecked(flags & 1);
        regex->setChecked(flags & 2);
        whole_word->setChecked(flags & 4);
        layout->addWidget(match_case); layout->addWidget(regex); layout->addWidget(whole_word);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
        layout->addWidget(buttons);
        QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
        query->setFocus(); query->selectAll();
        if (dialog.exec() == QDialog::Accepted && !v->is_closed()) {
            const uint32_t next = uint32_t(match_case->isChecked()) | (uint32_t(regex->isChecked()) << 1)
                                  | (uint32_t(whole_word->isChecked()) << 2);
            win->note(q(term_search_set(v->tab(), rs(query->text()), next))); v->update();
        }
        return true;
    }
    if (id == "search:find-next" || id == "search:find-previous") {
        if (v) {
            win->note(q(term_search_step(v->tab(), id == "search:find-next")));
            v->update();
        }
        return true;
    }
    if (id == "search:online") {
        if (!v) return true;
        const QString query = q(term_selected_text(v->tab())).trimmed();
        if (query.isEmpty()) { win->note(i18n::tr("请先选择要搜索的文本")); return true; }
        QUrl url(QStringLiteral("https://www.google.com/search"));
        QUrlQuery params; params.addQueryItem(QStringLiteral("q"), query); url.setQuery(params);
        QDesktopServices::openUrl(url);
        return true;
    }
    if (id == "view:any-panel") {
        ChromeDocks *d = win->docks();
        if (!d) return true;
        const QStringList labels = {i18n::tr("资源管理器"), i18n::tr("文件管理器"),
            i18n::tr("会话"), i18n::tr("历史命令"), i18n::tr("会话日志"),
            i18n::tr("AI 助手")};
        QDockWidget *panels[] = {d->conn, d->files, d->sess, d->history, d->hist, d->bottom};
        bool ok = false;
        const QString picked = QInputDialog::getItem(win, i18n::tr("任意面板"),
            i18n::tr("面板"), labels, 0, false, &ok);
        if (ok) {
            const int at = labels.indexOf(picked);
            if (at >= 0 && panels[at]) { panels[at]->show(); panels[at]->raise(); }
        }
        return true;
    }
    if (id == "view:command-palette") { show_command_palette(win); return true; }
    if (id == "view:theme") { run_prefs_dialog(win, 0); return true; }
    if (id == "view:highlight-caret-line") {
        win->setProperty("highlightCaretLine", action->isChecked());
        refresh_views(win); return true;
    }
    if (id == "view:zoom") {
        bool ok = false;
        const int px = QInputDialog::getInt(win, i18n::tr("终端字体缩放"),
            i18n::tr("字号（像素）"), int(term_font_px()), 10, 32, 1, &ok);
        if (ok && term_set_font_px(uint32_t(px))) {
            refresh_views(win);
            win->note(i18n::tr("终端字号已设为 %1 px").arg(px));
        }
        return true;
    }
    if (id == "mode:local" || id == "mode:remote") {
        if (v && !v->is_closed()) {
            term_set_reading(v->tab(), id == "mode:local");
            v->sync_reading_mode();
            win->refresh_chrome();
        }
        return true;
    }
    if (id == "mode:focus") { focus_toggle(win); return true; }
    if (id == "mode:focus-settings") { focus_settings(win); return true; }
    if (id == "mode:lock-screen") { chrome::lock_screen(win); return true; }
    if (id == "mode:lock-settings") { chrome::show_lock_settings(win); return true; }
    if (id == "mode:hex-edit") {
        if (v) {
            for (int i = 0; i < win->page_count(); ++i) {
                for (int pane = 0; PtyView *candidate = win->pane_view(i, pane); ++pane) {
                    if (candidate == v) {
                        win->page_sendbar(i)->set_hex_mode(action->isChecked());
                        return true;
                    }
                }
            }
        }
        return true;
    }
    if (id == "mode:fullscreen") {
        action->isChecked() ? win->showFullScreen() : win->showNormal(); return true;
    }
    if (id == "mode:always-on-top") {
        win->setWindowFlag(Qt::WindowStaysOnTopHint, action->isChecked());
        win->show(); return true;
    }
    if (id == "tools:sync-input") {
        win->show_sync_channels();
        return true;
    }
    if (id == "tools:onekeys") { run_onekey_dialog(win); return true; }
    if (id == "tools:sender") { win->new_sender_window(); return true; }
    if (id == "panel:log") {
        if (win->docks() && win->docks()->hist) {
            win->docks()->hist->show(); win->docks()->hist->raise();
        }
        return true;
    }
    if (id == "window:split-group-horizontal" || id == "window:split-group-vertical") {
        win->split_group(id == "window:split-group-horizontal" ? Qt::Horizontal : Qt::Vertical);
        return true;
    }
    if (id == "window:merge-to-group") { win->merge_current_group(); return true; }
    if (id == "window:close-group") { win->close_current_group(); return true; }
    if (id == "help:shortcuts") return show_shortcuts(win);
    if (id == "help:documentation" || id == "help:release-notes" || id == "help:report-issue") {
        QString suffix = QStringLiteral("/blob/master/README.md");
        if (id == "help:release-notes") suffix = QStringLiteral("/releases");
        if (id == "help:report-issue") suffix = QStringLiteral("/issues");
        QDesktopServices::openUrl(QUrl(QStringLiteral("https://gitee.com/xy2294490459/tide-term") + suffix));
        return true;
    }
    return false;
}
} // namespace chrome
