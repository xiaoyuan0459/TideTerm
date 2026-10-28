#include "cpp/chrome/docks.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/panel_header.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/shell_panel.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"
#include <QDockWidget>

namespace chrome {
void build_auxiliary_docks(MainWindow *win, ChromeDocks *d) {
    d->shells = new ShellPanel(win);
    d->shell = new QDockWidget(i18n::tr("Shell"), win);
    d->shell->setObjectName(QStringLiteral("dockShell"));
    d->shell->setWidget(d->shells);
    install_panel_header(d->shell, i18n::tr("Shell"))->set_dot(QStringLiteral("warn"));
    win->addDockWidget(Qt::BottomDockWidgetArea, d->shell);
    d->shell->hide();
    QObject::connect(d->shell, &QDockWidget::visibilityChanged, d->shells,
        [d](bool visible) { if (visible) d->shells->ensure_shell(); });

    d->transfer = new QDockWidget(i18n::tr("传输"), win);
    d->transfer->setObjectName(QStringLiteral("dockTransfer"));
    d->transfer->setWidget(d->sftp->transfer_widget());
    install_panel_header(d->transfer, i18n::tr("传输"))->set_dot(QStringLiteral("ok"));
    win->addDockWidget(Qt::BottomDockWidgetArea, d->transfer);
    d->transfer->hide();
    d->sftp->on_transfer_started = [d] { d->transfer->show(); d->transfer->raise(); };
}
}
