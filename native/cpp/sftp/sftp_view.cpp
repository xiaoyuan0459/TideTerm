// SftpBrowser 实现（Phase C ⑤ 真数据版；G3 的 FileTree 拖拽机制保留）。
//
// 与 Slint 版的结构性差异（G3 系数记录，仍然成立）：QTreeWidget 原生列头排序替掉
// SftpHeader + sort 回调；原生 QDrag/QMenu 替掉「方向性位移判定」DnD workaround；
// 行渲染 = QTreeWidgetItem 三行代码（SftpRow 类消失）。

#include "cpp/sftp/sftp_view.h"
#include "cpp/sftp/file_item.h"
#include "cpp/sftp/sftp_rows.h"

#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/elided_label.h"
#include "cpp/chrome/panel_splits.h"
#include "cpp/chrome/transient_scrollbars.h"

#include "cpp/sftp/sftp_columns.h"
#include "cpp/chrome/ui_metrics.h"
#include <QDesktopServices>

#include <QDir>
#include <QDrag>
#include <QDragEnterEvent>
#include <QDragLeaveEvent>
#include <QDropEvent>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QMimeData>
#include <QSplitter>
#include <QToolBar>
#include <QToolButton>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <QUrl>
#include <QVBoxLayout>

#include <cstdio>
#include <functional>

namespace {

constexpr const char *kRemoteMime = "application/x-tideterm-remote";

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 单栏文件树：名称、时间、类型、大小、权限及远程所有者/分组（WT 七列实测）。
class FileTree : public QTreeWidget {
public:
    FileTree(bool remote, QWidget *parent) : QTreeWidget(parent), remote_(remote) {
        setObjectName(QStringLiteral("fileTree"));
        // 列装配（标签/模式/宽度，含 Q-01 首显重放）集中在 sftp_columns.h。
        sftp::setup_file_tree_columns(this, remote);
        setRootIsDecorated(false);
        setUniformRowHeights(true);
        setAlternatingRowColors(false);
        setIconSize(QSize(16, 16));
        setContextMenuPolicy(Qt::CustomContextMenu);
        setDragEnabled(true);
        setAcceptDrops(true);
        setDropIndicatorShown(true);
        setSelectionMode(QAbstractItemView::ExtendedSelection);
        setSortingEnabled(true);
        sortItems(0, Qt::AscendingOrder);
        chrome::install_transient_scrollbars(this);
    }

    bool is_remote() const { return remote_; }

protected:
    bool widths_applied_ = false;

    /// Q-01：首显重放意图列宽（实现见 sftp_columns.h；此后交还用户交互）。
    void showEvent(QShowEvent *e) override {
        if (!widths_applied_) {
            widths_applied_ = true;
            sftp::apply_initial_column_widths(this, remote_);
        }
        QTreeWidget::showEvent(e);
    }

public: // 回复后续 std::function 回调成员的公开访问（on_drag_hint 等在此声明）

    /// 无 moc 构建链：回调用 std::function（省 moc 步骤——系数记录的一部分）。
    std::function<void(const QString &)> on_drag_hint;
    std::function<void()> on_transfer;
    /// SFTP-05：外部文件拖入（资源管理器/桌面），远程栏 = 上传、本地栏 = 复制进当前本地目录（非 file:// 的 URL 忽略）。
    std::function<void(const QStringList &)> on_external_drop;

    QMimeData *mimeData(const QList<QTreeWidgetItem *> &items) const override {
        Q_UNUSED(items);
        auto *m = new QMimeData;
        m->setData(kRemoteMime, remote_ ? "1" : "0");
        m->setText(remote_ ? QStringLiteral("remote-selection")
                           : QStringLiteral("local-selection"));
        return m;
    }

    void dragEnterEvent(QDragEnterEvent *e) override {
        // SFTP-05：接 OS 文件（text/uri-list）。此前只认自定义 mime，资源
        // 管理器拖入一律 ignore —— 文档记为特性缺口。
        if (e->mimeData()->hasUrls()) {
            e->acceptProposedAction();
            if (on_drag_hint) {
                on_drag_hint(remote_ ? i18n::tr("松开以将文件上传到远程")
                                     : i18n::tr("松开以将文件复制到本地目录"));
            }
            return;
        }
        if (e->mimeData()->hasFormat(kRemoteMime)) {
            e->acceptProposedAction();
            if (on_drag_hint) {
                on_drag_hint(remote_ ? i18n::tr("松开以将文件下载到本地")
                                     : i18n::tr("松开以将文件上传到远程"));
            }
            return;
        }
        e->ignore();
    }

    void dragLeaveEvent(QDragLeaveEvent *) override {
        if (on_drag_hint) {
            on_drag_hint(QString());
        }
    }

    void dropEvent(QDropEvent *e) override {
        if (e->mimeData()->hasUrls()) {
            e->acceptProposedAction();
            if (on_drag_hint) {
                on_drag_hint(QString());
            }
            if (on_external_drop) {
                QStringList files;
                const QList<QUrl> urls = e->mimeData()->urls();
                for (const QUrl &u : urls) {
                    const QString local = u.toLocalFile();
                    if (!local.isEmpty() && QFileInfo(local).isFile()) {
                        files << local;
                    }
                }
                if (!files.isEmpty()) {
                    on_external_drop(files);
                }
            }
            return;
        }
        if (e->mimeData()->hasFormat(kRemoteMime)) {
            e->acceptProposedAction();
            if (on_drag_hint) {
                on_drag_hint(QString());
            }
            if (on_transfer) {
                on_transfer();
            }
            return;
        }
        e->ignore();
    }

private:
    bool remote_;
};

} // namespace

// ---- SftpBrowser ----

SftpBrowser::SftpBrowser(QWidget *parent) : QWidget(parent) {
    setObjectName(QStringLiteral("sftpBrowser"));

    auto *col = new QVBoxLayout(this);
    col->setContentsMargins(0, 0, 0, 0);
    col->setSpacing(0);

    build_toolbar();

    // 面板内部双栏：与分屏同一套比例钳 + 不可折叠；比例跨重启保留
    // （横向排布时 resizeEvent 会翻取向，持久化侧对取向变化自动作废，见 panel_splits.h）。
    split_ = chrome::panel_splitter(Qt::Vertical, this);
    chrome::install_panel_split(split_);
    SftpPane remote, localv;
    build_pane(&remote, true);
    build_pane(&localv, false);
    remote_box_ = remote.box;
    local_box_ = localv.box;
    remote_path_strip_ = remote.path_strip;
    local_path_strip_ = localv.path_strip;
    remote_tree_ = remote.tree;
    remote_path_ = remote.path;
    local_tree_ = localv.tree;
    local_path_ = localv.path;
    split_->addWidget(remote.box);
    split_->addWidget(localv.box);
    col->addWidget(split_, 1);
    empty_ = new QLabel(i18n::tr("没有可用的文件管理器"), this);
    empty_->setObjectName(QStringLiteral("mutedHint"));
    empty_->setAlignment(Qt::AlignCenter);
    col->addWidget(empty_, 1);

    // ---- 传输队列（文件 / 方向 / 进度 / 状态） ----
    transfers_ = new QTreeWidget(this);
    transfers_->setColumnCount(4);
    transfers_->setHeaderLabels({i18n::tr("文件"), i18n::tr("方向"),
                                 i18n::tr("进度"), i18n::tr("状态")});
    transfers_->setRootIsDecorated(false);
    transfers_->setMaximumHeight(110);
    transfers_->hide();
    setup_transfer_actions();
    col->addWidget(transfers_);

    status_label_ = new chrome::ElidedLabel(this);
    status_label_->setText(i18n::tr("未连接"));
    status_label_->setObjectName(QStringLiteral("mutedHint"));
    col->addWidget(status_label_);

    // 选中变化 → 状态行统计（WT 行为）。
    const auto stats = [this] { refresh_stats(); };
    QObject::connect(remote_tree_, &QTreeWidget::itemSelectionChanged, this, stats);
    QObject::connect(local_tree_, &QTreeWidget::itemSelectionChanged, this, stats);

    // 本地栏起步：用户目录。
    scan_local(QDir::homePath());
    update_connection_view();
}

void SftpBrowser::resizeEvent(QResizeEvent *event) {
    QWidget::resizeEvent(event);
    if (split_ != nullptr) {
        split_->setOrientation(width() < 420 ? Qt::Vertical : Qt::Horizontal);
    }
}

void SftpBrowser::build_pane(SftpPane *p, bool remote) {
    p->box = new QWidget(this);
    auto *lay = new QVBoxLayout(p->box);
    lay->setContentsMargins(0, 0, 0, 0);
    lay->setSpacing(0);

    p->path_strip = new QWidget(p->box);
    p->path_strip->setObjectName(QStringLiteral("filePathRow"));
    p->path_strip->setAttribute(Qt::WA_StyledBackground, true);
    p->path_strip->setFixedHeight(chrome::metrics::kSectionHeaderHeight);
    auto *path_row = new QHBoxLayout(p->path_strip);
    path_row->setContentsMargins(2, 0, 3, 0);
    path_row->setSpacing(2);
    p->path = new QLineEdit(remote ? QStringLiteral("/") : QString(), p->path_strip);
    p->path->setObjectName(QStringLiteral("filePath"));
    auto *up = new QToolButton(p->path_strip);
    up->setObjectName(QStringLiteral("fileUp"));
    chrome::set_button_icon(up, QStringLiteral("up"), 12);
    up->setFixedSize(20, 20);
    up->setToolTip(i18n::tr("上级"));
    up->setAccessibleName(i18n::tr("上级"));
    path_row->addWidget(p->path, 1);
    path_row->addWidget(up);
    lay->addWidget(p->path_strip);

    auto *tree = new FileTree(remote, p->box);
    p->tree = tree;
    QLineEdit *path = p->path;
    lay->addWidget(tree, 1);

    // 路径栏回车 = 转到；上级按钮。
    QObject::connect(p->path, &QLineEdit::returnPressed, this, [this, remote, path] {
        if (remote) {
            if (!uid_.isEmpty()) {
                remote_navigate(path->text());
            }
        } else {
            scan_local(path->text());
        }
    });
    QObject::connect(up, &QToolButton::clicked, this, [this, remote, path] {
        QDir d(path->text());
        if (remote) {
            const QString cur = path->text();
            if (cur != QLatin1String("/") && !uid_.isEmpty()) {
                const QString parent = cur.left(cur.lastIndexOf('/'));
                remote_navigate(parent.isEmpty() ? QStringLiteral("/") : parent);
            }
        } else if (d.cdUp()) {
            scan_local(d.absolutePath());
        }
    });
    // 双击目录进入；跨栏拖拽 = 传输；右键 = 目录操作。
    QObject::connect(tree, &QTreeWidget::itemDoubleClicked, this,
                     [this, remote, path](QTreeWidgetItem *item, int) {
                         if (!item->data(0, Qt::UserRole + 1).toBool()) {
                             if (!remote) QDesktopServices::openUrl(QUrl::fromLocalFile(
                                 QDir(local_cwd_).filePath(item->data(0, Qt::UserRole).toString())));
                             return;
                         }
                         const QString name = item->data(0, Qt::UserRole).toString();
                         if (remote) {
                             if (!uid_.isEmpty()) {
                                 remote_navigate(remote_cwd_ == QLatin1String("/")
                                                     ? '/' + name
                                                     : remote_cwd_ + '/' + name);
                             }
                         } else {
                             scan_local(QDir(path->text()).filePath(name));
                         }
                     });
    tree->on_transfer = [this, remote] { start_transfer(!remote); };
    // SFTP-05：外部文件拖入 —— 远程栏上传到 remote_cwd_，本地栏复制进 local_cwd_
    //（语义实现在 handle_external_drop，公开给自检直测）。
    tree->on_external_drop = [this, remote](const QStringList &files) {
        handle_external_drop(files, remote);
    };
    QObject::connect(tree, &QTreeWidget::customContextMenuRequested, this,
                     [this, remote, tree](const QPoint &pos) {
                         do_context_menu(remote, tree->viewport()->mapToGlobal(pos));
                     });
}

void SftpBrowser::attach(const QString &uid, const QString &title, const QString &initial_dir) {
    context_tab_ = UINT32_MAX;
    if (!uid_.isEmpty()) remote_paths_[uid_] = remote_cwd_;
    uid_ = uid;
    remote_cwd_ = remote_paths_.value(uid, initial_dir.isEmpty() ? QStringLiteral(".") : initial_dir);
    browse_local_ = false;
    terminal_local_ = false;
    pending_open_ = true;
    status_ = i18n::tr("等待 %1 的 SSH 会话…").arg(title);
    status_label_->setText(status_);
    remote_tree_->clear();
    update_connection_view();
}

void SftpBrowser::detach(bool close_channel) {
    if (!uid_.isEmpty()) remote_paths_[uid_] = remote_cwd_;
    if (remote_paths_.size() > 64) remote_paths_.clear();
    if (close_channel && !uid_.isEmpty()) {
        sftp_close(rs(uid_));
    }
    uid_.clear();
    pending_open_ = false;
    remote_tree_->clear();
    status_ = i18n::tr("未连接");
    status_label_->setText(status_);
    update_connection_view();
}

void SftpBrowser::poll() {
    poll_local();
    if (pending_open_ && !uid_.isEmpty() && sftp_session_ready(rs(uid_))) {
        pending_open_ = false;
        status_ = i18n::tr("正在打开 SFTP…");
        status_label_->setText(status_);
        sftp_open(rs(uid_));
    }
    const rust::Vec<SftpOpPod> ops = sftp_take_ops();
    for (const SftpOpPod &op : ops) {
        if (q(op.uid) != uid_) {
            continue; // 旧会话的迟到结果丢弃
        }
        switch (op.kind) {
            case 5:
                status_ = i18n::tr("传输失败：%1").arg(q(op.error));
                break;
            case 0: // open
                if (op.ok) {
                    status_ = i18n::tr("SFTP 就绪");
                    remote_navigate(remote_cwd_);
                } else {
                    status_ = i18n::tr("SFTP 不可用：%1").arg(q(op.error));
                }
                break;
            case 1: // list
                if (q(op.path) != remote_cwd_) continue;
                if (op.ok) {
                    remote_tree_->clear();
                    remote_tree_->setSortingEnabled(false);
                    for (const SftpEntry &e : op.entries) {
                        auto *item = new FileItem(remote_tree_);
                        sftp_ui::fill_row(item, q(e.name), e.is_dir, e.size_bytes, e.permissions,
                                 e.modified_unix, q(e.owner), q(e.group));
                    }
                    remote_path_->setText(remote_cwd_);
                    remote_tree_->setSortingEnabled(true);
                    status_ = stats_text(remote_tree_);
                } else {
                    status_ = i18n::tr("列目录失败：%1").arg(q(op.error));
                }
                break;
            case 2:
                status_ = op.ok ? i18n::tr("目录已创建")
                                : i18n::tr("创建失败：%1").arg(q(op.error));
                remote_navigate(remote_cwd_);
                break;
            case 3:
                status_ = op.ok ? i18n::tr("已删除")
                                : i18n::tr("删除失败：%1").arg(q(op.error));
                remote_navigate(remote_cwd_);
                break;
            case 4:
                status_ = op.ok ? i18n::tr("已重命名")
                                : i18n::tr("重命名失败：%1").arg(q(op.error));
                remote_navigate(remote_cwd_);
                break;
            default:
                break;
        }
        status_label_->setText(status_);
        if (on_note && !op.ok) {
            on_note(status_);
        }
    }
    refresh_transfers();
}

void SftpBrowser::remote_navigate(const QString &path) {
    remote_cwd_ = path.isEmpty() ? QStringLiteral("/") : path;
    if (!uid_.isEmpty()) {
        sftp_list(rs(uid_), rs(remote_cwd_));
    }
}

int SftpBrowser::local_rows() const { return local_tree_->topLevelItemCount(); }
int SftpBrowser::remote_rows() const { return remote_tree_->topLevelItemCount(); }
int SftpBrowser::transfer_rows() const { return transfers_->topLevelItemCount(); }
QString SftpBrowser::status_text() const { return status_; }
QString SftpBrowser::attached_uid() const { return uid_; }
QTreeWidget *SftpBrowser::remote_tree() const { return remote_tree_; }
QTreeWidget *SftpBrowser::local_tree() const { return local_tree_; }
QToolBar *SftpBrowser::toolbar_widget() const { return toolbar_; }

/// 状态行统计（WT 实测口径）：无选中 `4400 项 110 MB` / 纯目录 `2 项`；
/// 选中 `1/4400 项 25 KB/110 MB`。大小只计文件（目录大小列为空）。
QString SftpBrowser::stats_text(QTreeWidget *tree) const {
    const int total = tree->topLevelItemCount();
    quint64 total_bytes = 0;
    for (int i = 0; i < total; ++i) {
        const QTreeWidgetItem *row = tree->topLevelItem(i);
        if (!row->data(0, Qt::UserRole + 1).toBool()) {
            total_bytes += row->data(0, Qt::UserRole + 2).toULongLong();
        }
    }
    int selected = 0;
    quint64 selected_bytes = 0;
    QTreeWidgetItemIterator it(tree, QTreeWidgetItemIterator::Selected);
    for (; *it != nullptr; ++it) {
        ++selected;
        if (!(*it)->data(0, Qt::UserRole + 1).toBool()) {
            selected_bytes += (*it)->data(0, Qt::UserRole + 2).toULongLong();
        }
    }
    QString text = selected > 0 ? i18n::tr("%1/%2 项").arg(selected).arg(total)
                                : i18n::tr("%1 项").arg(total);
    if (total_bytes > 0) {
        text += selected > 0
                    ? QStringLiteral(" %1/%2").arg(sftp_ui::size_label(false, selected_bytes),
                                                   sftp_ui::size_label(false, total_bytes))
                    : QStringLiteral(" %1").arg(sftp_ui::size_label(false, total_bytes));
    }
    return text;
}

/// 可见栏 = 已挂会话取远程，否则本地栏（空态由 empty_ 接管，不刷统计）。
void SftpBrowser::refresh_stats() {
    if (uid_.isEmpty() && !browse_local_) return;
    QTreeWidget *tree = uid_.isEmpty() ? local_tree_ : remote_tree_;
    status_ = stats_text(tree);
    status_label_->setText(status_);
}

// ---- G3 兼容包装（g3-selftest：结构断言 + 24 桩行） ----

QWidget *build_sftp_browser(QTreeWidget **out_tree, QToolBar **out_toolbar) {
    auto *browser = new SftpBrowser;
    browser->resize(1100, 720);
    browser->setWindowTitle(i18n::tr("SFTP 浏览器（G3 系数试点）"));
    // 桩行：远程栏填 24 示例（保住 G3 断言口径；真数据路径在 dock 内）。
    for (int i = 0; i < 24; ++i) {
        auto *r = new QTreeWidgetItem(browser->remote_tree());
        sftp_ui::fill_row(r, QStringLiteral("entry-%1").arg(i), i % 3 == 0, 4 * quint64(i), 0644,
                 1757700000 + i * 60);
    }
    if (out_tree) {
        *out_tree = browser->remote_tree();
    }
    if (out_toolbar) {
        *out_toolbar = browser->toolbar_widget();
    }
    return browser;
}
