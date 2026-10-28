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

void SftpBrowser::do_context_menu(bool remote, const QPoint &pos) {
    // WT 实测对齐（2026-09-29 实机菜单）：文件行与空白是两套——文件行多
    // 「打开/编辑器打开」与「复制名称/路径（含到终端）」组、无「新建」组；
    // 空白的 删除/重命名/移动到/下载/属性 置灰。缺后端的项保持置灰。
    QMenu menu(this);
    chrome::style_menu(&menu);
    QTreeWidget *tree = remote ? remote_tree_ : local_tree_;
    // WT 实测（2026-09-29）：右键落点决定目标——空白处即使残留 currentItem，
    // 也按无选中态弹（新建组出现、删除/重命名等置灰）。
    QTreeWidgetItem *sel = tree == nullptr ? nullptr : tree->itemAt(tree->viewport()->mapFromGlobal(pos));
    // 右键即选中落点行（WT 行为）：否则「下载/删除/重命名」会因 selectedItems()
    // 为空而静默早退，用户看起来像菜单无响应。
    if (sel != nullptr && tree != nullptr) {
        if (!sel->isSelected()) { tree->clearSelection(); sel->setSelected(true); }
        tree->setCurrentItem(sel, 0, QItemSelectionModel::NoUpdate);
    }
    const QString name = sel ? sel->data(0, Qt::UserRole).toString() : QString();
    const QString cwd = remote ? remote_cwd_ : local_cwd_;
    const bool connected = !uid_.isEmpty();
    const QString selected_uid = remote ? uid_ : QString();
    const QString upload_uid = uid_;
    const bool has_sel = sel != nullptr;
    const bool dir = has_sel && sel->data(0, Qt::UserRole + 1).toBool();
    const bool to_terminal = context_tab_ != UINT32_MAX;
    const auto source_tab = context_tab_;
    const QString full = has_sel
                             ? (remote ? (cwd == QLatin1String("/") ? QStringLiteral("/") + name
                                                                    : cwd + '/' + name)
                                       : QDir(cwd).filePath(name))
                             : QString();
    QStringList selected_paths;
    if (has_sel) for (auto *entry : tree->selectedItems()) {
        const auto selected_name = entry->data(0, Qt::UserRole).toString();
        selected_paths << (remote ? cwd + '/' + selected_name : QDir(cwd).filePath(selected_name));
    }
    const bool multiple = selected_paths.size() > 1;
    QStringList properties;
    if (has_sel) for (int column = 0; column < 7; ++column) properties << sel->text(column);

    QAction *refresh_a = chrome::add_action(&menu, QStringLiteral("refresh"),
        i18n::tr("刷新(F)"), true, [this] {
            scan_local(local_cwd_);
            if (!uid_.isEmpty()) {
                remote_navigate(remote_cwd_);
            }
        });
    refresh_a->setShortcut(QKeySequence(QStringLiteral("F5")));
    chrome::add_sep(&menu);
    chrome::add_action(&menu, QStringLiteral("folder"), i18n::tr("打开(O)"),
                       has_sel && (dir || !remote), [this, remote, dir, full] {
                           if (dir) {
                               if (remote) remote_navigate(full);
                               else scan_local(full);
                           } else {
                               QDesktopServices::openUrl(QUrl::fromLocalFile(full));
                           }
                       })->setShortcut(QKeySequence(QStringLiteral("Return")));
    chrome::add_action(&menu, QStringLiteral("file"), remote ? i18n::tr("在文本编辑器中编辑…") : i18n::tr("使用默认编辑器打开(E)"),
                       has_sel && !dir && (!remote || connected),
                       [this, remote, selected_uid, full] { if (remote) run_file_job(6, selected_uid, full, QString(), QString());
                           else QDesktopServices::openUrl(QUrl::fromLocalFile(full)); });
    chrome::add_sep(&menu);
    QAction *remove_a = chrome::add_action(&menu, QStringLiteral("delete"),
        i18n::tr("删除(R)"), (!remote || connected) && has_sel, nullptr);
    remove_a->setShortcut(QKeySequence(QStringLiteral("Del")));
    QAction *rename_a = chrome::add_action(&menu, QStringLiteral("rename"),
        i18n::tr("重命名…(N)"), (!remote || connected) && has_sel && !multiple, nullptr);
    rename_a->setShortcut(QKeySequence(QStringLiteral("F2")));
    chrome::add_action(&menu, QStringLiteral("move"), i18n::tr("移动到…(M)"), has_sel && (!remote || connected),
        [this, selected_uid, full, selected_paths, multiple] {
            if (multiple) { move_selected(selected_uid, selected_paths); return; }
            bool ok = false; const auto target = QInputDialog::getText(this, i18n::tr("移动到"),
            i18n::tr("目标绝对路径（不覆盖已有项目）"), QLineEdit::Normal, QString(), &ok);
            if (ok && !target.isEmpty()) run_file_job(2, selected_uid, full, selected_uid, target); })->setShortcut(QKeySequence(QStringLiteral("F6")));
    chrome::add_action(&menu, QStringLiteral("copy"), i18n::tr("复制到…"), has_sel && (!remote || connected),
        [this, selected_uid, full, selected_paths, multiple] {
            if (multiple) { copy_selected(selected_uid, selected_paths); return; }
            bool ok = false; const auto target = QInputDialog::getText(this, i18n::tr("复制到"),
            i18n::tr("目标绝对路径"), QLineEdit::Normal, QString(), &ok);
            if (ok && !target.isEmpty()) copy_tree(selected_uid, full, selected_uid, target); });
    chrome::add_sep(&menu);
    QAction *download_a = chrome::add_action(&menu, QStringLiteral("download"),
        i18n::tr("下载…(D)"), remote && connected && has_sel,
        [this] { start_transfer(false); });
    download_a->setShortcut(QKeySequence(QStringLiteral("F3")));
    QAction *upload_a = chrome::add_action(&menu, QStringLiteral("upload"),
        i18n::tr("上传到当前文件夹…(U)"), true, [this] { start_transfer(true); });
    upload_a->setShortcut(QKeySequence(QStringLiteral("F4")));
    chrome::add_action(&menu, QStringLiteral("upload"), i18n::tr("上传文件夹…"), connected,
        [this, upload_uid, remote_cwd = remote_cwd_] { const auto source = QFileDialog::getExistingDirectory(this, i18n::tr("选择上传文件夹"), local_cwd_);
            if (!source.isEmpty()) copy_tree(QString(), source, upload_uid,
                remote_cwd + '/' + QFileInfo(source).fileName()); });
    chrome::add_sep(&menu);
    QAction *mkdir_a = nullptr;
    if (!has_sel) {
        // WT：新建组只在空白菜单出现。
        mkdir_a = chrome::add_action(&menu, QStringLiteral("new_dir"),
            i18n::tr("新建目录…(D)"), !remote || connected, nullptr);
        mkdir_a->setShortcut(QKeySequence(QStringLiteral("Ctrl+Shift+N")));
        chrome::add_action(&menu, QStringLiteral("new_file"), i18n::tr("新建文件…(F)"),
                           !remote || connected, [this, selected_uid, cwd, remote] { const auto name = QInputDialog::getText(this, i18n::tr("新建文件"), i18n::tr("文件名"));
                               if (!name.isEmpty() && !name.contains('/') && !name.contains('\\')) run_file_job(1, QString(), QString(), selected_uid,
                                   remote ? cwd + '/' + name : QDir(cwd).filePath(name)); })->setShortcut(QKeySequence(QStringLiteral("Ctrl+N")));
        chrome::add_action(&menu, QStringLiteral("link"), i18n::tr("新建链接…(L)"),
                           !remote || connected, [this, selected_uid, cwd, remote] { const auto name = QInputDialog::getText(this, i18n::tr("新建链接"), i18n::tr("链接名称"));
                               if (name.isEmpty() || name.contains('/') || name.contains('\\')) return;
                               const auto target = QInputDialog::getText(this, i18n::tr("新建链接"), i18n::tr("目标绝对路径"));
                               if (!target.isEmpty()) run_file_job(5, selected_uid, target, selected_uid, remote ? cwd + '/' + name : QDir(cwd).filePath(name)); })->setShortcut(QKeySequence(QStringLiteral("Ctrl+L")));
        chrome::add_sep(&menu);
    }
    chrome::add_action(&menu, QStringLiteral("copy"), i18n::tr("复制目录路径(D)"),
                       !cwd.isEmpty(), [cwd] { host_clipboard::copy_text(cwd); });
    chrome::add_action(&menu, QStringLiteral("terminal"),
                       i18n::tr("将目录路径复制到终端(D)"), !cwd.isEmpty() && to_terminal,
                       [this, cwd, source_tab] {
                           insert_path(source_tab, cwd);
                       })->setShortcut(QKeySequence(QStringLiteral("F7")));
    if (has_sel) {
        chrome::add_sep(&menu);
        chrome::add_action(&menu, QStringLiteral("copy"), i18n::tr("复制名称(C)"), true,
                           [name] { host_clipboard::copy_text(name); })
            ->setShortcut(QKeySequence(QStringLiteral("Ctrl+C")));
        chrome::add_action(&menu, QStringLiteral("copy"), i18n::tr("复制路径(C)"), true,
                           [full] { host_clipboard::copy_text(full); })
            ->setShortcut(QKeySequence(QStringLiteral("Ctrl+Shift+C")));
        chrome::add_action(&menu, QStringLiteral("terminal"), i18n::tr("复制名称到终端(C)"),
                           to_terminal, [this, name, source_tab] {
                               insert_path(source_tab, name);
                           })->setShortcut(QKeySequence(QStringLiteral("Ctrl+P")));
        chrome::add_action(&menu, QStringLiteral("terminal"), i18n::tr("复制路径到终端(C)"),
                           to_terminal, [this, full, source_tab] {
                               insert_path(source_tab, full);
                           })->setShortcut(QKeySequence(QStringLiteral("Ctrl+Shift+P")));
    }
    chrome::add_sep(&menu);
    chrome::add_action(&menu, QStringLiteral("info"), i18n::tr("属性…(P)"), has_sel,
                       [this, remote, properties, name] {
                           if (properties.isEmpty()) {
                               return;
                           }
                           // 列序（2026-09-29）：1=修改 2=类型 3=大小，远程 4=权限 5=所有者 6=分组。
                           QString detail = QStringLiteral("%1\n%2：%3\n%4：%5\n%6：%7")
                                                .arg(name, i18n::tr("修改"), properties[1],
                                                     i18n::tr("类型"), properties[2],
                                                     i18n::tr("大小"), properties[3]);
                           if (remote) {
                               detail += QStringLiteral("\n%1：%2\n%3：%4\n%5：%6")
                                             .arg(i18n::tr("权限"), properties[4],
                                                  i18n::tr("所有者"), properties[5],
                                                  i18n::tr("分组"), properties[6]);
                           }
                           QMessageBox::information(this, i18n::tr("属性"), detail);
                       })->setShortcut(QKeySequence(QStringLiteral("Alt+Return")));

    QAction *picked = menu.exec(pos);
    if (picked == nullptr) {
        return;
    }
    if (remote && selected_uid.isEmpty()) {
        return;
    }
    const QString base = cwd == QLatin1String("/") ? QString('/') : cwd + '/';
    if (mkdir_a != nullptr && picked == mkdir_a) {
        const QString dir_name = QInputDialog::getText(
            this, i18n::tr("新建目录"), i18n::tr("目录名"));
        if (!dir_name.isEmpty()) {
            if (!dir_name.contains('/') && !dir_name.contains('\\')) run_file_job(0, QString(), QString(), selected_uid, base + dir_name);
        }
    } else if (picked == rename_a && sel != nullptr) {
        const QString new_name = QInputDialog::getText(
            this, i18n::tr("重命名"), i18n::tr("新名称"), QLineEdit::Normal, name);
        if (!new_name.isEmpty() && new_name != name) {
            if (!new_name.contains('/') && !new_name.contains('\\')) run_file_job(2, selected_uid, full, selected_uid, base + new_name);
        }
    } else if (picked == remove_a && sel != nullptr) {
        if (QMessageBox::question(this, i18n::tr("删除文件"), i18n::tr("删除所选项目及其子目录？此操作不可撤销。"),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes) {
            if (selected_paths.size() > 16) { status_label_->setText(i18n::tr("每批最多 16 个根项目，请缩小选择范围")); return; }
            for (const auto &path : selected_paths) run_file_job(3, selected_uid, path, QString(), QString());
        }
    }
}
