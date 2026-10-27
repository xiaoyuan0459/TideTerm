// Active pane and chrome context; UI focus state stays in Qt.
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/shell_panel.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/selection_actions.h"
#include "cpp/chrome/fold_regions.h"
#include "cpp/chrome/text_marks.h"
#include "cpp/chrome/log_actions.h"
#include "cpp/i18n.h"
#include <QAction>
#include "cpp/chrome/tab_workspace.h"
#include "cpp/chrome/tab_buttons.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QLabel>
#include <QTabWidget>

PtyView *MainWindow::active_view() const {
    if (docks_->shells && docks_->shells->owns_focus()) return docks_->shells->active_view();
    const int index = current_page();
    if (index < 0 || index >= int(page_roots_.size())) return nullptr;
    QWidget *root = page_roots_[size_t(index)];
    if (auto *focused = dynamic_cast<PtyView *>(QApplication::focusWidget());
        focused && is_ancestor(root, focused)) return focused;
    const auto remembered = active_panes_.value(root);
    if (remembered && is_ancestor(root, remembered)) return remembered;
    return first_view(root);
}

void MainWindow::refresh_chrome() {
    update_sync_scope();
    chrome::sync_tab_titles();
    chrome::refresh_log_actions(this);
    const bool auxiliary = docks_->shells && docks_->shells->owns_focus();
    for (const auto *id : {"window:split-view-horizontal", "window:split-view-vertical",
             "window:split-terminal-horizontal", "window:split-terminal-vertical",
             "window:split-group-horizontal", "window:split-group-vertical", "window:merge-to-group",
             "window:close-group", "window:zoom-pane", "mode:hex-edit"})
        if (auto *action = chrome::find_menu_action(menuBar(), QString::fromLatin1(id))) action->setEnabled(!auxiliary);
    for (int i = 0; i < int(page_roots_.size()); ++i) {
        // 横幅绑定页内活动窗格（无焦点回落页首窗格）。
        if (banners_[size_t(i)] != nullptr) {
            PtyView *target = active_panes_.value(page_roots_[size_t(i)]);
            if (!target || !is_ancestor(page_roots_[size_t(i)], target))
                target = first_view(page_roots_[size_t(i)]);
            if (target != nullptr) {
                banners_[size_t(i)]->bind(target->tab());
                if (auto *footer = page_roots_[size_t(i)]->findChild<QLabel *>(
                        QStringLiteral("sessionFooter"), Qt::FindDirectChildrenOnly)) {
                    const QString text = banners_[size_t(i)]->grid_text();
                    if (footer->text() != text) footer->setText(text);
                }
            }
        }
    }
    PtyView *active = active_view();
    const bool readable = active && !active->is_closed();
    const bool reading = readable && term_reading(active->tab());
    if (auto *action = chrome::find_menu_action(menuBar(), QStringLiteral("search:highlight"))) action->setEnabled(readable);
    const uint8_t find_selection = readable ? term_mark_seed(active->tab(), true).error : 7;
    for (const char *id : {"search:find-next-selection", "search:find-previous-selection"})
        if (auto *action = chrome::find_menu_action(menuBar(), QLatin1String(id))) {
            action->setEnabled(find_selection == 0); action->setToolTip(find_selection ? chrome::mark_error(find_selection) : QString());
        }
    if (auto *action = chrome::find_menu_action(menuBar(), QStringLiteral("view:fold-selection"))) {
        const uint8_t status = readable ? term_fold_selection_status(active->tab()) : 7;
        action->setEnabled(status == 0); action->setToolTip(chrome::fold_error(status));
    }
    if (auto *action = chrome::find_menu_action(menuBar(), QStringLiteral("view:fold-regions"))) {
        action->setEnabled(readable && term_fold_count(active->tab()) > 0);
        action->setToolTip(i18n::tr("当前没有可用的已完成命令输出"));
    }
    chrome::refresh_selection_actions(this, active);
    for (const auto *id : {"mode:local", "mode:remote"}) {
        if (auto *action = chrome::find_menu_action(menuBar(), QString::fromLatin1(id))) {
            action->setEnabled(readable);
            action->setChecked(readable && (QByteArray(id) == "mode:local" ? reading : !reading));
        }
    }
    setWindowTitle(active ? q_title(active->tab()) + QStringLiteral(" - TideTerm")
                          : QStringLiteral("TideTerm"));
    chrome::refresh_docks(docks_, active ? active->tab() : UINT32_MAX);
    // 文件管理器随活动会话（⑤）：attach 切换（uid 变了才重连）。
    if (docks_ != nullptr && docks_->sftp != nullptr) {
        PtyView *a = active_view();
        const SessCfg cfg = a ? sess_active_config(a->tab()) : SessCfg{};
        const rust::String uid = a != nullptr && cfg.kind == 1 && cfg.sftp_enabled
                                     ? term_uid(a->tab()) : rust::String();
        const QString q_uid = QString::fromUtf8(uid.data(), int(uid.size()));
        const uint32_t next_tab = a ? a->tab() : UINT32_MAX;
        if (next_tab != files_tab_ || q_uid != files_uid_) {
            files_tab_ = next_tab;
            files_uid_ = q_uid;
            if (q_uid.isEmpty()) {
                docks_->sftp->detach();
            } else {
                docks_->sftp->attach(q_uid, q_title(a->tab()), QString::fromUtf8(cfg.options.sftp_initial_dir.data(), int(cfg.options.sftp_initial_dir.size())));
            }
        }
        if (a) {
            const rust::String cwd = term_current_dir(a->tab());
            docks_->sftp->follow_terminal(a->tab(), cfg.kind,
                QString::fromUtf8(cwd.data(), int(cwd.size())), cfg.sftp_enabled, cfg.options.sftp_follow_cwd);
        }
    }
    refresh_status();
}
