#include "cpp/chrome/docks.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/log_actions.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QDockWidget>
#include <QDesktopServices>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QLabel>
#include <QMenu>
#include <QToolButton>
#include <QAction>
#include <QListView>
#include <QPushButton>
#include <QStringListModel>
#include <QUrl>
#include <QVBoxLayout>
namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
QDockWidget *make_dock(MainWindow *win, const QString &title, const QString &name, QWidget *body) {
    auto *dock = new QDockWidget(title, win); dock->setObjectName(name); dock->setWidget(body);
    dock->setFeatures(QDockWidget::DockWidgetMovable | QDockWidget::DockWidgetClosable | QDockWidget::DockWidgetFloatable);
    return dock;
}
}
namespace chrome {
/// 会话日志列：按 `log_filter` 重建；空态提示随之切换。
void apply_log_filter(ChromeDocks *d) {
    if (d == nullptr || d->log_labels == nullptr) {
        return;
    }
    QStringList rows;
    for (const QString &row : d->log_all)
        if (row.contains(d->log_filter, Qt::CaseInsensitive)) rows << row;
    d->log_labels->setStringList(rows);
    if (d->log_empty != nullptr) {
        d->log_empty->setVisible(rows.isEmpty());
    }
}

void build_log_dock(MainWindow *win, ChromeDocks *d) {
    // ---- 右下：活动会话的有界日志快照；关闭 dock 时不拉取正文。 ----
    auto *log_body = new QWidget(win);
    auto *log_layout = new QVBoxLayout(log_body);
    log_layout->setContentsMargins(4, 4, 4, 4);
    auto *log_tools = new QHBoxLayout;
    d->log_capture = new QPushButton(i18n::tr("开始记录"), log_body);
    auto *log_pause = new QPushButton(i18n::tr("暂停/恢复"), log_body);
    log_pause->setObjectName(QStringLiteral("logPause"));
    auto *more = new QToolButton(log_body);
    more->setText(i18n::tr("更多")); more->setPopupMode(QToolButton::InstantPopup);
    auto *menu = new QMenu(more); more->setMenu(menu);
    auto *log_settings = menu->addAction(i18n::tr("日志设置…"));
    auto *log_directory = menu->addAction(i18n::tr("日志目录"));
    auto *log_export = menu->addAction(i18n::tr("导出"));
    auto *log_clear = menu->addAction(i18n::tr("清空"));
    log_tools->addWidget(d->log_capture);
    log_tools->addWidget(log_pause);
    log_tools->addWidget(more);
    log_tools->addStretch(1);
    log_layout->addLayout(log_tools);
    auto *log_status = new QLabel(log_body);
    log_status->setTextFormat(Qt::PlainText);
    log_status->setObjectName(QStringLiteral("logDiskStatus"));
    log_status->setWordWrap(true);
    log_layout->addWidget(log_status);
    d->log_empty = new QLabel(i18n::tr("没有可用的会话日志"), log_body);
    d->log_empty->setAlignment(Qt::AlignCenter);
    d->log_empty->setObjectName(QStringLiteral("mutedHint"));
    log_layout->addWidget(d->log_empty);
    d->log_labels = new QStringListModel(log_body);
    auto *log_list = new QListView(log_body);
    log_list->setModel(d->log_labels);
    log_list->setSelectionMode(QAbstractItemView::ExtendedSelection);
    log_layout->addWidget(log_list, 1);
    d->hist = make_dock(win, i18n::tr("会话日志"), QStringLiteral("dockHist"), log_body);
    if (PanelHeader *head = chrome::install_panel_header(d->hist, i18n::tr("会话日志"),
                                                         i18n::tr("筛选"))) {
        head->set_dot(QStringLiteral("warn"));
        head->on_filter = [d](const QString &text) {
            d->log_filter = text;
            apply_log_filter(d);
        };
    }
    QObject::connect(d->log_capture, &QPushButton::clicked, win, [win] {
        if (PtyView *v = win->active_view()) {
            if (slog_active(v->tab())) slog_stop(v->tab());
            else slog_start(v->tab());
            win->refresh_chrome();
        }
    });
    QObject::connect(log_settings, &QAction::triggered, win, [win] { show_log_settings(win); });
    QObject::connect(log_clear, &QAction::triggered, win, [win] {
        if (PtyView *v = win->active_view()) {
            slog_clear(v->tab());
            win->refresh_chrome();
        }
    });
    QObject::connect(log_pause, &QPushButton::clicked, win, [win] {
        if (auto *view = win->active_view()) {
            const auto status = slog_status(view->tab());
            slog_pause(view->tab(), status.state != 3); win->refresh_chrome();
        }
    });
    QObject::connect(log_directory, &QAction::triggered, win, [win] {
        if (auto *view = win->active_view()) {
            const auto status = slog_status(view->tab());
            if (!status.path.empty()) {
                if (!QDesktopServices::openUrl(QUrl::fromLocalFile(q(status.path)))) win->note(i18n::tr("无法打开日志目录"));
            }
            else win->note(i18n::tr("当前会话尚未创建持续日志"));
        }
    });
    QObject::connect(log_export, &QAction::triggered, win, [win] {
        if (PtyView *v = win->active_view()) {
            try {
                const QString path = q(slog_export(v->tab()));
                QDesktopServices::openUrl(QUrl::fromLocalFile(QFileInfo(path).absolutePath()));
                win->note(i18n::tr("日志已导出：%1").arg(path));
            } catch (const std::exception &e) { win->note(QString::fromUtf8(e.what())); }
        }
    });

}
void refresh_log_dock(ChromeDocks *d, uint32_t active_tab) {
    if (active_tab == UINT32_MAX) {
        d->log_capture->setEnabled(false);
        d->log_all.clear(); apply_log_filter(d);
        d->log_tab = UINT32_MAX; d->log_revision = 0;
        if (auto *label = d->hist->findChild<QLabel *>(QStringLiteral("logDiskStatus"))) label->setText(i18n::tr("请先打开会话"));
        if (auto *button = d->hist->findChild<QPushButton *>(QStringLiteral("logPause"))) button->setEnabled(false);
        return;
    }
    d->log_capture->setText(slog_active(active_tab) ? i18n::tr("停止记录")
                                                    : i18n::tr("开始记录"));
    const auto disk = slog_status(active_tab);
    d->log_capture->setEnabled(disk.state != 4);
    if (disk.state == 6) {
        const QString failure = q(disk.path) + q(disk.error);
        if (d->hist->property("lastLogFailure").toString() != failure) {
            d->hist->setProperty("lastLogFailure", failure);
            if (auto *window = dynamic_cast<MainWindow *>(d->hist->parentWidget()))
                window->note(i18n::tr("日志写入失败，请打开会话日志查看详情"));
        }
    }
    if (auto *label = d->hist->findChild<QLabel *>(QStringLiteral("logDiskStatus"))) {
        const char *states[] = {"未记录到磁盘", "正在创建日志", "持续记录中", "日志已暂停", "正在排空日志", "日志已停止", "日志写入失败"};
        const QString text = i18n::tr(states[qMin(6, int(disk.state))]) + (disk.error.empty() ? QString() : QStringLiteral(": ") + q(disk.error))
            + (disk.path.empty() ? QString() : i18n::tr("\n%1\n保留 %2 MiB，累计写入 %3 MiB").arg(q(disk.path)).arg(double(disk.retained) / 1048576.0, 0, 'f', 2).arg(double(disk.written) / 1048576.0, 0, 'f', 2));
        if (label->text() != text) label->setText(text);
        label->setToolTip(q(disk.path));
    }
    if (auto *button = d->hist->findChild<QPushButton *>(QStringLiteral("logPause"))) button->setEnabled(disk.state == 2 || disk.state == 3);
    if (!d->hist->isVisible()) return;
    const uint64_t revision = slog_revision(active_tab);
    if (d->log_tab == active_tab && d->log_revision == revision) return;
    d->log_tab = active_tab;
    d->log_revision = revision;
    const QString recent = q(slog_recent(active_tab, 100));
    d->log_all = recent.isEmpty() ? QStringList() : recent.split(QChar('\n'));
    apply_log_filter(d);
}
}
