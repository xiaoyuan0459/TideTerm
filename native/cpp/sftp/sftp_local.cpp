// Local navigation is asynchronous; terminal CWD changes supersede pending scans.
#include "cpp/sftp/sftp_view.h"
#include "cpp/sftp/file_item.h"
#include "cpp/sftp/sftp_rows.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/file_icons.h"
#include "cpp/i18n.h"
#include <QDateTime>
#include <QDir>
#include <QLabel>
#include <QLineEdit>
#include <QTreeWidget>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
}

SftpBrowser::~SftpBrowser() { files_close(files_owner_); }

void SftpBrowser::follow_terminal(uint32_t tab, uint8_t kind, const QString &cwd, bool sftp_enabled, bool follow_cwd) {
    const bool switched = context_tab_ != tab;
    const bool moved = context_cwd_ != cwd;
    const bool disabled = kind == 1 && !sftp_enabled;
    if (!switched && !moved && disabled == session_sftp_disabled_) return;
    session_sftp_disabled_ = disabled;
    context_tab_ = tab;
    context_cwd_ = cwd;
    terminal_local_ = kind == 0;
    browse_local_ = terminal_local_;
    if (terminal_local_) scan_local(cwd.isEmpty() ? QDir::homePath() : cwd);
    else if (kind == 1 && follow_cwd && !cwd.isEmpty() && cwd != remote_cwd_) {
        remote_cwd_ = cwd;
        if (!pending_open_ && sftp_channel_ready(rs(uid_))) remote_navigate(cwd);
    }
    update_connection_view();
}

void SftpBrowser::scan_local(const QString &dir) {
    // QDir path normalization is lexical; no directory enumeration on the UI thread.
    local_cwd_ = QDir::cleanPath(dir);
    local_path_->setText(QDir::toNativeSeparators(local_cwd_));
    local_tree_->clear();
    if (!files_owner_) files_owner_ = files_create();
    if (!files_owner_) {
        status_ = i18n::tr("文件浏览器数量已达到上限");
        status_label_->setText(status_);
        return;
    }
    files_request(files_owner_, rs(local_cwd_));
    if (uid_.isEmpty()) {
        status_ = i18n::tr("正在读取目录…");
        status_label_->setText(status_);
    }
}

void SftpBrowser::poll_local() {
    if (!files_owner_) return;
    const auto snapshot = files_poll(files_owner_);
    if (!snapshot.ready || q(snapshot.path) != local_cwd_) return;
    local_tree_->setUpdatesEnabled(false);
    local_tree_->setSortingEnabled(false);
    local_tree_->clear();
    for (const auto &entry : snapshot.entries) {
        auto *item = new FileItem(local_tree_);
        const QString name = q(entry.name);
        item->setText(0, name);
        item->setIcon(0, chrome::file_type_icon(name, entry.is_dir));
        item->setText(1, entry.modified > 0 ? QDateTime::fromSecsSinceEpoch(entry.modified)
            .toString(QStringLiteral("yyyy/MM/dd HH:mm:ss")) : QString());
        item->setText(2, chrome::file_type_label(name, entry.is_dir));
        item->setText(3, sftp_ui::size_label(entry.is_dir, entry.size));
        item->setTextAlignment(3, Qt::AlignRight | Qt::AlignVCenter);
        item->setData(0, Qt::UserRole, name);
        item->setData(0, Qt::UserRole + 1, entry.is_dir);
        item->setData(0, Qt::UserRole + 2, QVariant::fromValue(qulonglong(entry.size)));
    }
    local_tree_->setSortingEnabled(true);
    local_tree_->setUpdatesEnabled(true);
    if (uid_.isEmpty()) {
        status_ = stats_text(local_tree_);
        if (!snapshot.error.empty()) {
            status_ = i18n::tr("无法读取目录：%1").arg(q(snapshot.error));
        } else if (snapshot.truncated) {
            status_ += i18n::tr(" · 仅显示前 10000 项");
        }
        status_label_->setText(status_);
    }
}
