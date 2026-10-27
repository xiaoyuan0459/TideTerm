#include "cpp/chrome/fold_regions.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QDialog>
#include <QAction>
#include <QMap>
#include <QDialogButtonBox>
#include <QHeaderView>
#include <QLabel>
#include <QPointer>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>

QString chrome::fold_error(uint8_t code) {
    switch (code) {
    case 0: return i18n::tr("折叠所选输出");
    case 1: return i18n::tr("请先选择连续的多行输出");
    case 2: return i18n::tr("仅支持一个流式选区，不能使用矩形或多重选择");
    case 3: return i18n::tr("请选择同一已完成命令内至少两行输出");
    case 4: return i18n::tr("范围重复或交叉，请选择完整嵌套或独立范围");
    case 5: return i18n::tr("最多保留 128 个手动范围，嵌套深度最多 8 层");
    case 6: return i18n::tr("锁定期间不能修改折叠范围");
    default: return i18n::tr("折叠来源已失效，请重新打开");
    }
}

bool chrome::dispatch_fold(MainWindow *window, QAction *action) {
    const QByteArray id = action->property("tid").toByteArray();
    if (id == "view:fold-regions") { show_fold_regions(window); return true; }
    if (id != "view:fold-selection" && id != "view:fold") return false;
    auto *view = window->active_view();
    if (!view || view->is_closed()) return true;
    if (id == "view:fold-selection") {
        const auto result = term_fold_selection(view->tab());
        window->note(result.error ? fold_error(result.error) : i18n::tr("已添加折叠范围"));
    } else if (!term_command_fold_current(view->tab())) window->note(i18n::tr("当前位置没有可折叠的多行输出"));
    view->sync_reading_mode(); view->update(); window->refresh_chrome();
    return true;
}

void chrome::show_fold_regions(MainWindow *window) {
    QPointer<PtyView> view = window->active_view();
    if (!view || view->is_closed() || window->property("screenLocked").toBool()) return;
    const uint32_t tab = view->tab();
    const auto generation = term_history_state(tab).generation;
    QDialog dialog(window);
    dialog.setObjectName(QStringLiteral("foldRegions"));
    dialog.setWindowTitle(i18n::tr("折叠区域")); dialog.resize(880, 520);
    auto *layout = new QVBoxLayout(&dialog);
    auto *tree = new QTreeWidget(&dialog);
    tree->setObjectName(QStringLiteral("foldTree"));
    tree->setHeaderLabels({i18n::tr("范围"), i18n::tr("行"), i18n::tr("状态")});
    tree->setUniformRowHeights(true);
    tree->setTextElideMode(Qt::ElideRight);
    tree->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    tree->header()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    tree->header()->setSectionResizeMode(2, QHeaderView::ResizeToContents);
    auto *status = new QLabel(i18n::tr("折叠只改变显示；移除范围不会删除终端文字"), &dialog);
    status->setTextFormat(Qt::PlainText); status->setWordWrap(true);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);
    buttons->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
    const auto button = [&](const QString &label, const char *name) {
        auto *value = buttons->addButton(label, QDialogButtonBox::ActionRole);
        value->setObjectName(QLatin1String(name)); return value;
    };
    auto *open = button(i18n::tr("展开"), "foldOpen");
    auto *close = button(i18n::tr("折叠"), "foldClose");
    auto *open_all = button(i18n::tr("递归展开"), "foldOpenRecursive");
    auto *close_all = button(i18n::tr("递归折叠"), "foldCloseRecursive");
    auto *jump = button(i18n::tr("定位"), "foldJump");
    auto *remove = button(i18n::tr("移除范围"), "foldRemove");
    auto *refresh = button(i18n::tr("刷新"), "foldRefresh");
    layout->addWidget(tree, 1); layout->addWidget(status); layout->addWidget(buttons);
    const auto ready = [&] { return view && !view->is_closed() && !window->property("screenLocked").toBool() &&
        term_history_state(tab).available && term_history_state(tab).generation == generation; };
    const auto selected = [&]() -> uint64_t { return tree->currentItem() ? tree->currentItem()->data(0, Qt::UserRole).toULongLong() : 0; };
    const auto select = [&] {
        auto *item = tree->currentItem(); const bool valid = ready() && item;
        open->setEnabled(valid && item->data(0, Qt::UserRole + 2).toBool());
        close->setEnabled(valid && !item->data(0, Qt::UserRole + 2).toBool());
        open_all->setEnabled(valid); close_all->setEnabled(valid); jump->setEnabled(valid);
        remove->setEnabled(valid && item->data(0, Qt::UserRole + 1).toBool());
    };
    const auto reload = [&] {
        const uint64_t old = selected(); tree->clear();
        if (ready()) {
            QMap<uint64_t, QTreeWidgetItem *> items;
            for (const auto &row : term_fold_rows(tab)) {
                auto *item = new QTreeWidgetItem();
                item->setText(0, row.title.empty() && !row.manual ? i18n::tr("连接启动输出") : QString::fromUtf8(row.title.data(), int(row.title.size())));
                item->setToolTip(0, item->text(0));
                item->setText(1, QStringLiteral("%1–%2").arg(row.first).arg(row.last));
                item->setText(2, row.folded ? i18n::tr("已折叠") : i18n::tr("已展开"));
                item->setData(0, Qt::UserRole, QVariant::fromValue(qulonglong(row.id)));
                item->setData(0, Qt::UserRole + 1, row.manual);
                item->setData(0, Qt::UserRole + 2, row.folded);
                if (auto *parent = items.value(row.parent)) parent->addChild(item); else tree->addTopLevelItem(item);
                items.insert(row.id, item);
                if (row.id == old) tree->setCurrentItem(item);
            }
            tree->expandAll();
            if (!tree->currentItem() && tree->topLevelItemCount()) tree->setCurrentItem(tree->topLevelItem(0));
        }
        if (!ready()) status->setText(i18n::tr("折叠来源已失效，请重新打开"));
        else if (!tree->topLevelItemCount()) status->setText(i18n::tr("当前没有可用的已完成命令输出"));
        select();
    };
    const auto change = [&](bool folded, bool recursive) {
        if (!ready() || !term_fold_set(tab, selected(), folded, recursive)) {
            status->setText(i18n::tr("折叠来源已失效，请重新打开")); reload(); return;
        }
        view->sync_reading_mode(); view->update(); window->refresh_chrome(); reload();
    };
    const auto locate = [&] {
        if (!ready() || !term_fold_jump(tab, selected())) { reload(); return; }
        view->sync_reading_mode(); view->update(); window->refresh_chrome(); dialog.accept(); view->setFocus();
    };
    QObject::connect(tree, &QTreeWidget::currentItemChanged, &dialog, select);
    QObject::connect(tree, &QTreeWidget::itemActivated, &dialog, locate);
    QObject::connect(open, &QPushButton::clicked, &dialog, [&] { change(false, false); });
    QObject::connect(close, &QPushButton::clicked, &dialog, [&] { change(true, false); });
    QObject::connect(open_all, &QPushButton::clicked, &dialog, [&] { change(false, true); });
    QObject::connect(close_all, &QPushButton::clicked, &dialog, [&] { change(true, true); });
    QObject::connect(jump, &QPushButton::clicked, &dialog, locate);
    QObject::connect(refresh, &QPushButton::clicked, &dialog, reload);
    QObject::connect(remove, &QPushButton::clicked, &dialog, [&] {
        if (ready() && term_fold_remove(tab, selected())) {
            view->sync_reading_mode(); view->update(); window->refresh_chrome();
        }
        reload();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    QObject::connect(view, &QObject::destroyed, &dialog, &QDialog::reject);
    reload(); tree->setFocus(); dialog.exec();
}
