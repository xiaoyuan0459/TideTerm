#include "cpp/clipboard.h"
// A bounded snapshot from Rust; the dialog never retains terminal output history.
#include "cpp/chrome/command_outline.h"
#include "cpp/chrome/terminal_preview.h"
#include "cpp/chrome/hover_preview.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QClipboard>
#include <QDialog>
#include <QDialogButtonBox>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPointer>
#include <QPushButton>
#include <QScrollArea>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &text) { return QString::fromUtf8(text.data(), int(text.size())); }
}

void chrome::show_command_outline(MainWindow *window) {
    QPointer<PtyView> view(window->active_view());
    if (!view || view->is_closed()) return;
    const uint32_t tab = view->tab();
    QDialog dialog(window);
    dialog.setObjectName(QStringLiteral("commandOutline"));
    dialog.setWindowTitle(i18n::tr("命令大纲"));
    dialog.resize(940, 560);
    auto *layout = new QVBoxLayout(&dialog);
    auto *filter = new QLineEdit(&dialog);
    filter->setObjectName(QStringLiteral("outlineFilter"));
    filter->setPlaceholderText(i18n::tr("筛选命令"));
    auto *list = new QListWidget(&dialog);
    list->setObjectName(QStringLiteral("outlineList"));
    list->setUniformItemSizes(true);
    list->setTextElideMode(Qt::ElideRight);
    list->setMouseTracking(true);
    list->viewport()->setMouseTracking(true);
    auto *hover = new HoverPreview(list->viewport(), view);
    auto *status = new QLabel(&dialog);
    status->setTextFormat(Qt::PlainText);
    auto *preview = new QLabel(&dialog);
    preview->setObjectName(QStringLiteral("outlinePreview"));
    preview->setTextFormat(Qt::PlainText);
    preview->setWordWrap(true);
    preview->setTextInteractionFlags(Qt::TextSelectableByMouse | Qt::TextSelectableByKeyboard);
    auto *preview_area = new QScrollArea(&dialog);
    preview_area->setWidgetResizable(true);
    preview_area->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    preview_area->setMinimumHeight(100);
    preview_area->setMaximumHeight(230);
    preview_area->setWidget(preview);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);
    buttons->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
    auto *jump = buttons->addButton(i18n::tr("定位命令"), QDialogButtonBox::ActionRole);
    auto *copy_command = buttons->addButton(i18n::tr("复制命令"), QDialogButtonBox::ActionRole);
    auto *copy_output = buttons->addButton(i18n::tr("复制输出"), QDialogButtonBox::ActionRole);
    auto *fold = buttons->addButton(i18n::tr("折叠输出"), QDialogButtonBox::ActionRole);
    auto *insert = buttons->addButton(i18n::tr("填入发送栏"), QDialogButtonBox::ActionRole);
    auto *refresh = buttons->addButton(i18n::tr("刷新"), QDialogButtonBox::ActionRole);
    auto *detach = buttons->addButton(i18n::tr("独立预览"), QDialogButtonBox::ActionRole);
    detach->setObjectName(QStringLiteral("outlineDetachPreview"));
    jump->setObjectName(QStringLiteral("outlineJump"));
    copy_command->setObjectName(QStringLiteral("outlineCopyCommand"));
    copy_output->setObjectName(QStringLiteral("outlineCopyOutput"));
    fold->setObjectName(QStringLiteral("outlineFold"));
    insert->setObjectName(QStringLiteral("outlineInsert"));
    layout->addWidget(filter);
    layout->addWidget(list, 1);
    layout->addWidget(status);
    layout->addWidget(preview_area);
    layout->addWidget(buttons);
    const auto ready = [&] { return view && !view->is_closed() && view->tab() == tab; };
    const auto selected_id = [&]() -> uint64_t {
        auto *item = list->currentItem();
        return item && !item->isHidden() ? item->data(Qt::UserRole).toULongLong() : 0;
    };
    const auto select = [&] {
        const uint64_t id = selected_id();
        const bool valid = ready() && id != 0;
        bool startup = false;
        jump->setEnabled(valid); copy_command->setEnabled(valid); insert->setEnabled(valid);
        const CommandText output = valid ? term_command_output(tab, id, true) : CommandText{};
        copy_output->setEnabled(valid && output.available);
        detach->setEnabled(valid && output.available);
        fold->setEnabled(false);
        if (valid) for (const auto &row : term_commands(tab)) if (row.id == id) {
            startup = row.startup;
            copy_command->setEnabled(!row.startup); insert->setEnabled(!row.startup);
            fold->setEnabled(row.foldable);
            fold->setText(row.folded ? i18n::tr("展开输出") : i18n::tr("折叠输出"));
        }
        preview->setText(q(output.text));
        status->setText(!valid ? i18n::tr("没有可定位的命令；请先执行命令")
            : !output.available ? i18n::tr("命令位置已失效，请刷新")
            : output.truncated ? i18n::tr("预览已截断；复制输出最多保留 64 KiB")
            : startup ? i18n::tr("启动输出可查看、复制和折叠，不作为命令发送")
            : i18n::tr("定位和复制不会发送；填入发送栏后可编辑再发送"));
    };
    const auto apply_filter = [&] {
        hover->cancel();
        QListWidgetItem *first = nullptr;
        for (int at = 0; at < list->count(); ++at) {
            auto *item = list->item(at);
            item->setHidden(!item->data(Qt::UserRole + 1).toString().contains(filter->text(), Qt::CaseInsensitive));
            if (!item->isHidden() && !first) first = item;
        }
        if (!list->currentItem() || list->currentItem()->isHidden()) list->setCurrentItem(first);
        select();
    };
    const auto reload = [&] {
        hover->cancel();
        const uint64_t old = selected_id();
        list->clear();
        if (ready()) for (const auto &row : term_commands(tab)) {
            const QString title = row.startup ? i18n::tr("连接启动输出") : q(row.title);
            auto *item = new QListWidgetItem(QStringLiteral("%1  %2%3").arg(row.line).arg(title)
                .arg(row.folded ? i18n::tr("（已折叠）") : row.finished ? QString() : i18n::tr("（执行中）")), list);
            item->setData(Qt::UserRole, QVariant::fromValue(qulonglong(row.id)));
            item->setData(Qt::UserRole + 1, title);
            item->setToolTip(title);
            if (row.id == old) list->setCurrentItem(item);
        }
        apply_filter();
    };
    const auto locate = [&] {
        if (!ready() || !term_command_jump(tab, selected_id())) { reload(); return; }
        view->sync_reading_mode(); view->update(); window->refresh_chrome(); dialog.accept();
    };
    QObject::connect(filter, &QLineEdit::textChanged, &dialog, apply_filter);
    QObject::connect(list, &QListWidget::currentItemChanged, &dialog, select);
    QObject::connect(list, &QListWidget::viewportEntered, &dialog, [hover] { hover->cancel(); });
    QObject::connect(list, &QListWidget::itemEntered, &dialog, [=](QListWidgetItem *item) {
        if (!ready() || !item || item->isHidden()) { hover->cancel(); return; }
        const uint64_t id = item->data(Qt::UserRole).toULongLong();
        const QString title = item->data(Qt::UserRole + 1).toString();
        const QPoint point = list->viewport()->mapToGlobal(list->visualItemRect(item).topRight());
        hover->request(id, point, [tab, id, title] {
            const auto output = term_command_output(tab, id, true);
            return !output.available ? i18n::tr("命令位置已失效，请刷新") :
                i18n::tr("命令预览：%1").arg(title.left(240)) + QLatin1Char('\n') + q(output.text) +
                (output.truncated ? QLatin1Char('\n') + i18n::tr("预览已限制为 8 KiB 或 32 行") : QString());
        });
    });
    QObject::connect(list, &QListWidget::itemActivated, &dialog, locate);
    QObject::connect(jump, &QPushButton::clicked, &dialog, locate);
    QObject::connect(refresh, &QPushButton::clicked, &dialog, reload);
    QObject::connect(detach, &QPushButton::clicked, &dialog, [&] {
        if (!ready()) return;
        const uint64_t id = selected_id();
        dialog.accept();
        show_terminal_preview(window, view, id);
    });
    QObject::connect(copy_command, &QPushButton::clicked, &dialog, [&] {
        if (!ready()) return;
        for (const auto &row : term_commands(tab)) if (row.id == selected_id()) {
            if (!row.startup) host_clipboard::copy_text(q(row.title)); return;
        }
        reload();
    });
    QObject::connect(copy_output, &QPushButton::clicked, &dialog, [&] {
        if (!ready()) return;
        const CommandText output = term_command_output(tab, selected_id(), false);
        if (!output.available) { reload(); return; }
        if (!host_clipboard::copy_text(q(output.text))) {
            status->setText(i18n::tr("剪贴板为空或不可用")); return;
        }
        status->setText(output.truncated ? i18n::tr("输出已复制（已截断到 64 KiB 或 1024 行）") : i18n::tr("输出已复制"));
    });
    QObject::connect(fold, &QPushButton::clicked, &dialog, [&] {
        if (!ready()) return;
        for (const auto &row : term_commands(tab)) if (row.id == selected_id()) {
            term_command_fold(tab, row.id, !row.folded);
            view->sync_reading_mode(); view->update(); window->refresh_chrome(); break;
        }
        reload();
    });
    QObject::connect(insert, &QPushButton::clicked, &dialog, [&] {
        if (!ready() || window->active_view() != view) return;
        for (const auto &row : term_commands(tab)) if (row.id == selected_id()) {
            if (row.startup) return;
            if (auto *bar = window->page_sendbar(window->current_page())) {
                bar->set_hex_mode(false); bar->input()->setText(q(row.title)); bar->show();
                dialog.accept(); bar->input()->setFocus();
            }
            return;
        }
        reload();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    reload(); filter->setFocus(); dialog.exec();
}
