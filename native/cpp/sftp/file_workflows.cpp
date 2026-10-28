#include "cpp/sftp/sftp_view.h"
#include <QTreeWidget>
#include <QDir>
#include <QFileInfo>
#include <QLineEdit>
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QDesktopServices>
#include <QDialog>
#include <QVBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QPointer>
#include <QUrl>
#include <QInputDialog>
#include <QMessageBox>
#include <QFileDialog>
#include <QProcess>
#include <QStandardPaths>
#include <exception>

namespace {
rust::String r(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool open_editor(QWidget *parent, const QString &path) {
    QString program;
#ifdef Q_OS_WIN
    program = QStandardPaths::findExecutable(QStringLiteral("notepad.exe"), {qEnvironmentVariable("WINDIR") + QStringLiteral("/System32")});
#endif
    if (program.isEmpty()) program = QFileDialog::getOpenFileName(parent, i18n::tr("选择文本编辑器程序"));
    return !program.isEmpty() && QProcess::startDetached(program, {path});
}
void show_error(QWidget *parent, const QString &title, const QString &text) {
    QMessageBox dialog(QMessageBox::Warning, title, text, QMessageBox::Ok, parent);
    dialog.setTextFormat(Qt::PlainText); dialog.exec();
}
class EditDialog final : public QDialog {
public:
    using QDialog::QDialog;
    std::function<bool()> may_close;
    void reject() override { if (!may_close || may_close()) QDialog::reject(); }
};
}

void SftpBrowser::copy_tree(const QString &source_uid, const QString &source, const QString &target_uid, const QString &target) {
    bool ok = false;
    const QStringList choices{i18n::tr("不覆盖（冲突时停止）"), i18n::tr("跳过已有文件"), i18n::tr("替换已有文件")};
    const auto selected = QInputDialog::getItem(this, i18n::tr("文件冲突策略"), target, choices, 0, false, &ok);
    if (ok) run_file_job(4, source_uid, source, target_uid, target, uint8_t(choices.indexOf(selected)));
}

void SftpBrowser::copy_selected(const QString &uid, const QStringList &sources) {
    if (sources.isEmpty()) return;
    if (sources.size() > 16) { show_error(this, i18n::tr("复制到"), i18n::tr("每批最多 16 个根项目，请缩小选择范围")); return; }
    const QString target = QInputDialog::getText(this, i18n::tr("复制到"), i18n::tr("目标目录（绝对路径）"), QLineEdit::Normal, uid.isEmpty() ? local_cwd_ : remote_cwd_);
    if (target.isEmpty()) return;
    bool ok = false;
    const QStringList choices{i18n::tr("不覆盖（冲突时停止）"), i18n::tr("跳过已有文件"), i18n::tr("替换已有文件")};
    const auto policy = QInputDialog::getItem(this, i18n::tr("文件冲突策略"), target, choices, 0, false, &ok);
    if (!ok) return;
    for (const auto &source : sources) run_file_job(4, uid, source, uid,
        uid.isEmpty() ? QDir(target).filePath(QFileInfo(source).fileName()) : target + '/' + QFileInfo(source).fileName(), uint8_t(choices.indexOf(policy)));
}

void SftpBrowser::move_selected(const QString &uid, const QStringList &sources) {
    if (sources.isEmpty()) return;
    if (sources.size() > 16) { show_error(this, i18n::tr("移动到"), i18n::tr("每批最多 16 个根项目，请缩小选择范围")); return; }
    bool ok = false;
    const QString target = QInputDialog::getText(this, i18n::tr("移动到"),
        i18n::tr("目标目录（绝对路径，不覆盖已有项目）"), QLineEdit::Normal, uid.isEmpty() ? local_cwd_ : remote_cwd_, &ok);
    if (!ok || target.isEmpty()) return;
    for (const auto &source : sources) run_file_job(2, uid, source, uid,
        uid.isEmpty() ? QDir(target).filePath(QFileInfo(source).fileName()) : target + '/' + QFileInfo(source).fileName());
}

void SftpBrowser::run_file_job(uint8_t op, const QString &source_uid, const QString &source,
    const QString &target_uid, const QString &target, uint8_t policy) {
    try {
        if (!files_owner_) files_owner_ = files_create();
        const uint64_t owner = files_owner_;
        const uint64_t id = files_task_start(owner, op, r(source_uid), r(source), r(target_uid), r(target), policy);
        int retained = 0;
        for (int i = transfers_->topLevelItemCount() - 1; i >= 0; --i) {
            auto *entry = transfers_->topLevelItem(i);
            if (entry->data(0, Qt::UserRole + 9).toBool() && entry->data(0, Qt::UserRole + 1).toBool() && ++retained > 48)
                delete transfers_->takeTopLevelItem(i);
        }
        auto *entry = new QTreeWidgetItem(transfers_);
        entry->setText(0, target.isEmpty() ? source : target);
        const char *operations[] = {"新建目录", "新建文件", "移动", "删除", "复制", "新建链接", "下载编辑副本", "回传编辑副本"};
        entry->setText(1, i18n::tr(operations[qMin(7, int(op))]));
        entry->setText(3, i18n::tr("排队/处理中"));
        entry->setData(0, Qt::UserRole, QStringLiteral("workflow-%1").arg(id));
        entry->setData(0, Qt::UserRole + 1, false);
        entry->setData(0, Qt::UserRole + 9, true);
        entry->setData(0, Qt::UserRole + 10, QVariant::fromValue(qulonglong(owner)));
        entry->setData(0, Qt::UserRole + 11, QVariant::fromValue(qulonglong(id)));
        entry->setToolTip(3, i18n::tr("目录任务支持取消；已提交的文件保留。普通文件传输可单独暂停/续传。"));
        update_transfers_visibility();
        if (on_transfer_started) on_transfer_started();
        auto *timer = new QTimer(this);
        QObject::connect(timer, &QTimer::timeout, this, [this, owner, id, entry, timer, op] {
            const auto task = files_task_poll(owner, id);
            entry->setText(2, i18n::tr("%1 / %2，%3 字节").arg(task.done).arg(task.total).arg(task.bytes));
            if (task.state == 1) {
                entry->setToolTip(0, q(task.path));
                return;
            }
            timer->stop(); timer->deleteLater();
            entry->setData(0, Qt::UserRole + 1, true);
            entry->setText(3, task.state == 2 ? i18n::tr("完成") : task.state == 4 ? i18n::tr("已取消（保留已完成项目）") : i18n::tr("失败：%1").arg(q(task.error)));
            entry->setToolTip(3, q(task.error));
            if (task.state == 2) {
                scan_local(local_cwd_); if (!uid_.isEmpty()) remote_navigate(remote_cwd_);
                if (op == 6) edit_remote(task.edit, q(task.path));
                else if (on_note) on_note(i18n::tr("文件操作已完成"));
            } else if (task.state == 3 || task.state == 4) {
                if (on_note) on_note(i18n::tr("文件操作未完成，请查看传输列表"));
            }
        });
        timer->start(100);
    } catch (const std::exception &error) {
        show_error(this, i18n::tr("文件操作未开始"), QString::fromUtf8(error.what()));
    }
}

void SftpBrowser::edit_remote(uint64_t edit, const QString &path) {
    auto *dialog = new EditDialog(this);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(i18n::tr("编辑远端文件"));
    auto *layout = new QVBoxLayout(dialog);
    auto *label = new QLabel(i18n::tr("在外部编辑器保存后，点击检查并回传。远端文件变化时拒绝覆盖。"), dialog);
    label->setWordWrap(true); layout->addWidget(label);
    auto *file = new QLabel(path, dialog); file->setTextInteractionFlags(Qt::TextSelectableByMouse); layout->addWidget(file);
    file->setTextFormat(Qt::PlainText);
    auto *open = new QPushButton(i18n::tr("打开编辑副本"), dialog); layout->addWidget(open);
    auto *upload = new QPushButton(i18n::tr("检查并回传"), dialog); layout->addWidget(upload);
    auto *close = new QPushButton(i18n::tr("关闭编辑副本"), dialog); layout->addWidget(close);
    const uint64_t owner = files_owner_;
    QObject::connect(open, &QPushButton::clicked, dialog, [dialog, path] { open_editor(dialog, path); });
    QObject::connect(upload, &QPushButton::clicked, dialog, [this, edit] { run_file_job(7, QString(), QString::number(edit), QString(), QString()); });
    dialog->may_close = [dialog, owner, edit] {
        if (QMessageBox::question(dialog, i18n::tr("关闭编辑副本"),
            i18n::tr("确认关闭外部编辑器并移除临时副本？未回传的修改会丢失。"),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes) return false;
        try { files_edit_close(owner, edit, true); return true; }
        catch (const std::exception &error) { show_error(dialog, i18n::tr("无法关闭编辑副本"), QString::fromUtf8(error.what())); return false; }
    };
    QObject::connect(close, &QPushButton::clicked, dialog, [dialog] { dialog->reject(); });
    dialog->resize(560, 220); dialog->show();
    if (!open_editor(dialog, path)) label->setText(i18n::tr("无法启动文本编辑器，请手动打开上面的临时文件。"));
}
