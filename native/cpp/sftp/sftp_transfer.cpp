#include "cpp/clipboard.h"
#include "cpp/sftp/sftp_view.h"

#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/menu_style.h"

#include <QAction>
#include <QClipboard>
#include <QDesktopServices>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QGuiApplication>
#include <QHeaderView>
#include <QInputDialog>
#include <QItemSelectionModel>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QPair>
#include <QSet>
#include <QTimer>
#include <QTreeWidget>
#include <QUrl>
#include <QVector>

namespace {
QString text(const rust::String &value) {
    return QString::fromUtf8(value.data(), int(value.size()));
}

rust::String rust_text(const QString &value) {
    return rust::String(value.toUtf8().toStdString());
}

QString transfer_size(quint64 bytes) {
    const char *units[] = {"B", "KB", "MB", "GB", "TB"};
    double value = double(bytes);
    int unit = 0;
    while (value >= 1024.0 && unit < 4) {
        value /= 1024.0;
        ++unit;
    }
    return QStringLiteral("%1 %2").arg(value, 0, 'f', unit == 0 ? 0 : 1)
        .arg(QString::fromLatin1(units[unit]));
}
} // namespace

QWidget *SftpBrowser::transfer_widget() { transfers_->setMaximumHeight(QWIDGETSIZE_MAX); transfers_->show(); return transfers_; }

void SftpBrowser::setup_transfer_actions() {
    transfers_->setContextMenuPolicy(Qt::CustomContextMenu);
    QObject::connect(transfers_, &QTreeWidget::customContextMenuRequested, this,
                     [this](const QPoint &position) {
                         auto *item = transfers_->itemAt(position);
                         if (item == nullptr) {
                             return;
                         }
                         // 空态占位行（NoItemFlags）不进任务菜单。
                         if (!item->flags().testFlag(Qt::ItemIsSelectable)) {
                             return;
                         }
                         QMenu menu(this);
                         const bool completed = item->data(0, Qt::UserRole + 1).toBool();
                         if (item->data(0, Qt::UserRole + 9).toBool()) {
                             auto *cancel_job = menu.addAction(i18n::tr("取消目录/文件操作"));
                             cancel_job->setEnabled(!completed);
                             if (menu.exec(transfers_->viewport()->mapToGlobal(position)) == cancel_job)
                                 files_task_cancel(item->data(0, Qt::UserRole + 10).toULongLong(), item->data(0, Qt::UserRole + 11).toULongLong());
                             return;
                         }
                         auto *pause = completed ? nullptr : menu.addAction(i18n::tr("暂停传输"));
                         auto *resume = completed && item->data(0, Qt::UserRole + 2).toBool()
                                            ? menu.addAction(i18n::tr("继续传输")) : nullptr;
                         auto *cancel = completed ? nullptr : menu.addAction(i18n::tr("取消传输"));
                         auto *picked = menu.exec(transfers_->viewport()->mapToGlobal(position));
                         if (picked == nullptr) {
                             return;
                         }
                         const auto id = rust_text(item->data(0, Qt::UserRole).toString());
                         if (picked == pause) {
                             sftp_pause_transfer(id);
                         } else if (picked == resume) {
                             sftp_resume_transfer(id);
                         } else if (picked == cancel) {
                             sftp_cancel_transfer(id);
                         }
                     });
}

void SftpBrowser::refresh_transfers() {
    const quint64 version = sftp_transfers_version();
    if (version == transfers_seen_ ||
        (transfers_refresh_clock_.isValid() && transfers_refresh_clock_.elapsed() < 100)) {
        return;
    }
    transfers_seen_ = version;
    transfers_refresh_clock_.restart();
    transfers_->setUpdatesEnabled(false);
    QSet<QString> present;
    bool upload_finished = false; // Q-03：本帧有上传成功完成 → 远端列表待刷新
    for (const TransferPod &transfer : sftp_transfers()) {
        const bool finished_now = transfer.completed && transfer.error.empty()
                                  && !notified_done_.contains(text(transfer.id));
        if (finished_now) {
            notified_done_.insert(text(transfer.id));
            if (transfer.up) upload_finished = true;
        }
        const QString id = text(transfer.id);
        present.insert(id);
        auto *item = transfer_items_.value(id, nullptr);
        if (item == nullptr) {
            item = new QTreeWidgetItem(transfers_);
            transfer_items_.insert(id, item);
            item->setText(0, text(transfer.file_name));
            item->setText(1, transfer.up ? i18n::tr("↑ 上传") : i18n::tr("↓ 下载"));
            item->setData(0, Qt::UserRole, id);
        }
        const int percent = transfer.bytes_total > 0
                                ? int(transfer.bytes_done * 100 / transfer.bytes_total)
                                : 0;
        const QString amount = QStringLiteral("%1 / %2 (%3%)")
                                   .arg(transfer_size(transfer.bytes_done),
                                        transfer_size(transfer.bytes_total))
                                   .arg(percent);
        const QString state = transfer.completed
                                  ? (transfer.error.empty()
                                         ? i18n::tr("完成")
                                         : text(transfer.error) == QLatin1String("transfer paused")
                                               ? i18n::tr("已暂停")
                                               : i18n::tr("失败：%1").arg(text(transfer.error)))
                                  : i18n::tr("传输中");
        if (item->text(2) != amount) item->setText(2, amount);
        if (item->text(3) != state) item->setText(3, state);
        if (item->data(0, Qt::UserRole + 1).toBool() != transfer.completed)
            item->setData(0, Qt::UserRole + 1, transfer.completed);
        const bool resumable = transfer.completed && !transfer.error.empty()
                               && text(transfer.error) != QLatin1String("transfer cancelled");
        if (item->data(0, Qt::UserRole + 2).toBool() != resumable)
            item->setData(0, Qt::UserRole + 2, resumable);
    }
    for (auto it = transfer_items_.begin(); it != transfer_items_.end();) {
        if (present.contains(it.key())) {
            ++it;
        } else {
            auto *item = it.value();
            delete transfers_->takeTopLevelItem(transfers_->indexOfTopLevelItem(item));
            it = transfer_items_.erase(it);
        }
    }
    transfers_->setUpdatesEnabled(true);
    update_transfers_visibility();
    // 上传落地后远端目录内容已变化：立即重扫当前目录（此前需手动重进才可见）。
    if (upload_finished && !uid_.isEmpty()) {
        remote_navigate(remote_cwd_);
    }
}

/// 传输面板可见性（WT 实测 2026-10-06）：有任务即显示；无任务时仅当
/// ⚙「传输面板」常驻（对齐 WT Ctrl+T 的传输 dock）才显示，并给出
/// 「没有传输」空态行；两者都不满足则隐藏。
void SftpBrowser::update_transfers_visibility() {
    // 占位行标记：NoItemFlags，不进右键菜单语义（菜单只对真任务生效）。
    const QString kPlaceholder = QString(QChar(0x01)) + QStringLiteral("placeholder");
    int placeholder_at = -1;
    for (int i = 0; i < transfers_->topLevelItemCount(); ++i) {
        if (transfers_->topLevelItem(i)->data(0, Qt::UserRole).toString() == kPlaceholder) {
            placeholder_at = i;
            break;
        }
    }
    const int real_rows = transfers_->topLevelItemCount() - (placeholder_at >= 0 ? 1 : 0);
    if (placeholder_at >= 0 && real_rows > 0) {
        delete transfers_->takeTopLevelItem(placeholder_at);
        placeholder_at = -1;
    } else if (placeholder_at < 0 && real_rows == 0 && transfers_pinned_) {
        auto *row = new QTreeWidgetItem(transfers_);
        row->setData(0, Qt::UserRole, kPlaceholder);
        row->setText(0, i18n::tr("没有传输"));
        row->setFlags(Qt::NoItemFlags);
    }
    const bool show = real_rows > 0 || transfers_pinned_;
    transfers_->setVisible(show);
}

void SftpBrowser::set_transfers_pinned(bool pinned) {
    transfers_pinned_ = pinned;
    // 常驻模式解除 110px 上限，让多任务列表可读；取消常驻恢复紧凑。
    transfers_->setMaximumHeight(pinned ? QWIDGETSIZE_MAX : 110);
    update_transfers_visibility();
}

// ---- 传输发起 / 右键目录操作 / 外部拖入（自 sftp_view.cpp 拆入：行数门禁） ----

void SftpBrowser::start_transfer(bool upload) {
    if (uid_.isEmpty()) {
        status_ = i18n::tr("未连接会话（SFTP 需 SSH 会话）");
        status_label_->setText(status_);
        if (on_note) {
            on_note(status_);
        }
        return;
    }
    const QString source_uid = uid_;
    const QString source_cwd = remote_cwd_;
    const auto source_tab = context_tab_;
    const auto source_generation = completion_context(source_tab).generation;
    const auto source_valid = [this, source_uid, source_cwd, source_tab, source_generation] {
        return uid_ == source_uid && remote_cwd_ == source_cwd && context_tab_ == source_tab
            && completion_context(source_tab).generation == source_generation;
    };
    if (upload) {
        const QStringList files =
            QFileDialog::getOpenFileNames(this, i18n::tr("选择上传文件"), local_cwd_);
        if (!source_valid()) { if (on_note) on_note(i18n::tr("来源会话已变化，请重新选择路径")); return; }
        enqueue_uploads(files);
    } else {
        const QList<QTreeWidgetItem *> selected = remote_tree_->selectedItems();
        if (selected.isEmpty()) {
            status_ = i18n::tr("先在远程栏选中一个文件");
            status_label_->setText(status_);
            return;
        }
        struct SelectedFile { QString name; bool directory; quint64 bytes; };
        QVector<SelectedFile> entries;
        for (auto *entry : selected) entries.push_back({entry->data(0, Qt::UserRole).toString(),
            entry->data(0, Qt::UserRole + 1).toBool(), entry->data(0, Qt::UserRole + 2).toULongLong()});
        // WT 实测（2026-10-06）：右键「下载」先弹「选择本地目录」目录树对话框，
        // 取消即中止；确认后所选目录作为落地目录（不静默写入本地窗格当前目录）。
        const QString picked_dir = QFileDialog::getExistingDirectory(
            this, i18n::tr("选择本地目录"), local_cwd_);
        if (picked_dir.isEmpty()) {
            return;
        }
        if (!source_valid()) { if (on_note) on_note(i18n::tr("来源会话已变化，请重新选择路径")); return; }
        const QString base =
            remote_cwd_ == QLatin1String("/") ? QString('/') : remote_cwd_ + '/';
        QVector<QPair<QString, quint64>> requests;
        for (const auto &entry : entries) {
            const QString name = entry.name;
            if (entry.directory) {
                copy_tree(uid_, base + name, QString(), QDir(picked_dir).filePath(name)); continue;
            }
            requests.append({base + name, entry.bytes});
        }
        const QString uid = uid_;
        const QString local_dir = picked_dir;
        auto *timer = new QTimer(this);
        QObject::connect(timer, &QTimer::timeout, this,
                         [this, timer, requests, uid, local_dir, source_valid, index = 0]() mutable {
                             if (!source_valid()) {
                                 timer->stop(); timer->deleteLater();
                                 status_label_->setText(i18n::tr("来源会话已变化，请重新选择路径"));
                                 if (on_note) on_note(status_label_->text()); return;
                             }
                             const int end = qMin(index + 128, int(requests.size()));
                             for (; index < end; ++index) {
                                 const auto &request = requests[index];
                                 sftp_download(rust_text(uid), rust_text(request.first),
                                               rust_text(local_dir), request.second);
                             }
                             if (index == int(requests.size())) { timer->stop(); timer->deleteLater(); }
                         });
        timer->start(1);
        if (!requests.isEmpty() && on_transfer_started) on_transfer_started();
    }
}

void SftpBrowser::enqueue_uploads(const QStringList &files) {
    if (files.isEmpty()) return;
    const QString uid = uid_;
    const QString remote_dir = remote_cwd_;
    const auto source_tab = context_tab_;
    const auto source_generation = completion_context(source_tab).generation;
    auto *timer = new QTimer(this);
    QObject::connect(timer, &QTimer::timeout, this,
                     [this, timer, files, uid, remote_dir, source_tab, source_generation, index = 0]() mutable {
                         if (uid_ != uid || context_tab_ != source_tab || completion_context(source_tab).generation != source_generation) {
                             timer->stop(); timer->deleteLater();
                             status_label_->setText(i18n::tr("来源会话已变化，请重新选择路径"));
                             if (on_note) on_note(status_label_->text()); return;
                         }
                         const int end = qMin(index + 128, int(files.size()));
                         for (; index < end; ++index) {
                             sftp_upload(rust_text(uid), rust_text(files[index]),
                                         rust_text(remote_dir));
                         }
                         if (index == int(files.size())) { timer->stop(); timer->deleteLater(); }
                     });
    timer->start(1);
    if (on_transfer_started) on_transfer_started();
}

// SFTP-05：外部文件拖入的目标语义（FileTree::dropEvent 连线；公开给自检）。
void SftpBrowser::handle_external_drop(const QStringList &files, bool to_remote) {
    if (files.isEmpty()) {
        return;
    }
    if (to_remote) {
        if (uid_.isEmpty()) {
            status_ = i18n::tr("未连接会话（SFTP 需 SSH 会话）");
            status_label_->setText(status_);
            if (on_note) {
                on_note(status_);
            }
            return;
        }
        enqueue_uploads(files);
        status_ = i18n::tr("已加入上传队列：%1 个文件").arg(files.size());
    } else {
        if (files.size() > 16) { status_label_->setText(i18n::tr("每批最多 16 个根项目，请缩小选择范围")); return; }
        bool skipped_self = false;
        for (const QString &f : files) {
            const QString dest = QDir(local_cwd_).filePath(QFileInfo(f).fileName());
            if (QFileInfo(f).absoluteFilePath() == QFileInfo(dest).absoluteFilePath()) { skipped_self = true; continue; }
            run_file_job(4, QString(), f, QString(), dest);
        }
        status_ = skipped_self ? i18n::tr("已跳过源与目标相同的项目，其余复制请求请查看传输列表")
            : i18n::tr("本地复制请求已提交，请查看传输列表");
    }
    status_label_->setText(status_);
}
