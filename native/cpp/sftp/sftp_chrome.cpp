// Compact file-manager toolbar and disconnected state; no background scans.
#include "cpp/sftp/sftp_view.h"

#include <QAction>
#include <QLabel>
#include <QLayout>
#include <QLineEdit>
#include <QMenu>
#include <QSplitter>
#include <QToolBar>
#include <QToolButton>
#include <QTreeWidget>

#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/i18n.h"

void SftpBrowser::build_toolbar() {
    toolbar_ = new QToolBar(this);
    toolbar_->setObjectName(QStringLiteral("fileTools"));
    toolbar_->setMovable(false);
    toolbar_->setToolButtonStyle(Qt::ToolButtonIconOnly);
    toolbar_->setIconSize(QSize(14, 14));
    toolbar_->setFixedHeight(chrome::metrics::kPanelFilterRowHeight);
    toolbar_->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    auto action = [this](const QString &glyph, const QString &text, auto callback) {
        auto *a = toolbar_->addAction(chrome::scalable_icon(glyph, 14), text);
        a->setProperty("chromeIcon", glyph);
        connect(a, &QAction::triggered, this, callback);
        return a;
    };
    action(QStringLiteral("refresh"), i18n::tr("刷新"), [this] {
        if (browse_local_ || !uid_.isEmpty()) scan_local(local_cwd_);
        if (!uid_.isEmpty()) remote_navigate(remote_cwd_);
    });
    auto *more = new QToolButton(toolbar_);
    chrome::set_button_icon(more, QStringLiteral("more"));
    more->setToolTip(i18n::tr("更多"));
    more->setPopupMode(QToolButton::InstantPopup);
    auto *menu = new QMenu(more);
    chrome::style_menu(menu);
    auto *local = menu->addAction(i18n::tr("本地文件"));
    local->setCheckable(true);
    connect(local, &QAction::toggled, this, [this](bool on) {
        browse_local_ = on;
        if (on) scan_local(local_cwd_);
        update_connection_view();
    });
    auto *panel = menu->addAction(i18n::tr("传输面板"));
    panel->setCheckable(true);
    connect(panel, &QAction::toggled, this, [this](bool on) {
        set_transfers_pinned(on);
    });
    menu->addSeparator();
    auto *upload = menu->addAction(i18n::tr("上传…"));
    auto *download = menu->addAction(i18n::tr("下载选中…"));
    connect(upload, &QAction::triggered, this, [this] { start_transfer(true); });
    connect(download, &QAction::triggered, this, [this] { start_transfer(false); });
    auto *disconnect = menu->addAction(i18n::tr("断开"));
    connect(disconnect, &QAction::triggered, this, [this] { detach(true); });
    connect(menu, &QMenu::aboutToShow, this, [this, upload, download, disconnect] {
        for (auto *a : {upload, download, disconnect}) a->setEnabled(!uid_.isEmpty());
    });
    more->setMenu(menu);
    toolbar_->addWidget(more);
}

void SftpBrowser::update_connection_view() {
    const bool attached = !uid_.isEmpty();
    empty_->setText(session_sftp_disabled_ ? i18n::tr("此会话已关闭 SFTP") : i18n::tr("没有可用的文件管理器"));
    QWidget *path_strip = attached ? remote_path_strip_ : local_path_strip_;
    if (toolbar_->parentWidget() != path_strip) {
        path_strip->layout()->addWidget(toolbar_);
        toolbar_->show();
    }
    split_->setVisible(attached || browse_local_);
    remote_box_->setVisible(attached);
    local_box_->setVisible(browse_local_);
    empty_->setVisible(!attached && !browse_local_);
    status_label_->setVisible(attached || browse_local_);
}
