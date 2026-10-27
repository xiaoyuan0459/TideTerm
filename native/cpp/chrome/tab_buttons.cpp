#include "cpp/chrome/tab_buttons.h"

#include <QLabel>
#include <QTabBar>
#include <QTabWidget>
#include <QToolButton>
#include <QApplication>
#include <algorithm>
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/icon_label.h"
#include "cpp/chrome/display_names.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/chrome/tab_workspace.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

namespace chrome {
namespace {

constexpr const char *kSquare = "tabActivitySquare";
constexpr const char *kLead = "tabStatusLead";
constexpr const char *kCloseSlot = "tabCloseSlot";

} // namespace

void sync_tab_titles() {
    struct Tab { QTabWidget *group; int at; uint32_t id; QString key; };
    std::vector<Tab> tabs;
    QHash<QString, std::vector<uint32_t>> instances;
    for (auto *widget : QApplication::topLevelWidgets()) {
        auto *window = dynamic_cast<MainWindow *>(widget);
        if (!window || !window->workspace()) continue;
        for (auto *group : window->workspace()->groups()) {
            for (int at = 0; at < group->count(); ++at) {
                const auto views = MainWindow::views(group->widget(at));
                if (views.empty() || views.front()->is_closed()) continue;
                const uint32_t id = views.front()->tab();
                const auto cfg = sess_active_config(id);
                const QString key = cfg.id.empty() ? QStringLiteral("live:%1").arg(id)
                    : QStringLiteral("saved:") + QString::fromUtf8(cfg.id.data(), int(cfg.id.size()));
                tabs.push_back({group, at, id, key});
                instances[key].push_back(id);
            }
        }
    }
    for (auto &ids : instances) std::sort(ids.begin(), ids.end());
    for (const auto &tab : tabs) {
        const auto &ids = instances[tab.key];
        QString title = MainWindow::q_title(tab.id);
        if (ids.size() > 1) {
            const auto at = std::lower_bound(ids.begin(), ids.end(), tab.id) - ids.begin();
            title += QStringLiteral(" (%1)").arg(qlonglong(at + 1));
        }
        if (tab.group->tabText(tab.at) != title) tab.group->setTabText(tab.at, title);
    }
}

void sync_tab_buttons(MainWindow *win) {
    for (auto *group : win->workspace()->groups()) {
        QTabBar *bar = group->tabBar();
        for (int i = 0; i < bar->count(); ++i) {
            QWidget *right = bar->tabButton(i, QTabBar::RightSide);
            if (!right || right->objectName() != QLatin1String(kCloseSlot)) {
                auto *slot = new QWidget(bar);
                slot->setObjectName(QLatin1String(kCloseSlot));
                slot->setFixedSize(metrics::kTabCloseButtonSize + metrics::kTabCloseInset,
                                   metrics::kTabCloseButtonSize);
                auto *close = new QToolButton(slot);
                close->setObjectName(QStringLiteral("tabClose"));
                close->setFixedSize(metrics::kTabCloseButtonSize, metrics::kTabCloseButtonSize);
                close->setToolTip(i18n::tr("关闭标签"));
                close->setFocusPolicy(Qt::NoFocus);
                set_button_icon(close, QStringLiteral("close"), 12);
                QObject::connect(close, &QToolButton::clicked, win, [win, group, bar, slot] {
                    for (int at = 0; at < bar->count(); ++at) {
                        if (bar->tabButton(at, QTabBar::RightSide) == slot) {
                            request_close_tab(win, win->page_index(group->widget(at)));
                            return;
                        }
                    }
                });
                bar->setTabButton(i, QTabBar::RightSide, slot);
            }
            QWidget *lead = bar->tabButton(i, QTabBar::LeftSide);
            if (!lead || lead->objectName() != QLatin1String(kLead)) {
                lead = new QWidget(bar);
                lead->setObjectName(QLatin1String(kLead));
                lead->setFixedSize(metrics::kTabLeadInset + metrics::kTabActivitySize,
                                   metrics::kTabCloseButtonSize);
                lead->setAttribute(Qt::WA_TransparentForMouseEvents);
                auto *square = new IconLabel(lead);
                square->setObjectName(QLatin1String(kSquare));
                square->setGeometry(metrics::kTabLeadInset,
                                    (metrics::kTabCloseButtonSize - metrics::kTabActivitySize) / 2,
                                    metrics::kTabActivitySize, metrics::kTabActivitySize);
                bar->setTabButton(i, QTabBar::LeftSide, lead);
            }
            auto *square = static_cast<IconLabel *>(lead->findChild<QLabel *>(QLatin1String(kSquare)));

            const uint32_t id = win->page_id(win->page_index(group->widget(i)));
            // UINT32_MAX = 页内还没窗格（建页/拆页中间态），不能过桥。
            const StatusKind kind = status_kind(id == UINT32_MAX ? 0 : term_status_kind(id));
            const QString status = QLatin1String(kind.name);

            square->setGeometry(metrics::kTabLeadInset,
                (metrics::kTabCloseButtonSize - metrics::kTabActivitySize) / 2,
                metrics::kTabActivitySize, metrics::kTabActivitySize);
            const QString style = QStringLiteral("background: transparent;");
            if (square->styleSheet() != style) square->setStyleSheet(style);
            const auto cfg = id == UINT32_MAX ? SessCfg{} : sess_active_config(id);
            square->set_glyph(id == UINT32_MAX ? QStringLiteral("terminal") : session_icon(cfg), metrics::kTabActivitySize);
            square->set_ink(cfg.kind == 0 ? QColor() : activity_color(status));
            const QString hint = i18n::tr(kind.text);
            square->setAccessibleName(hint);
            const QString tooltip = QStringLiteral("%1 · %2").arg(bar->tabText(i), hint);
            if (bar->tabToolTip(i) != tooltip) bar->setTabToolTip(i, tooltip);
        }
    }
}

} // namespace chrome
