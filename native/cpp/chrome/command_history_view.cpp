#include "cpp/clipboard.h"
#include "cpp/chrome/command_history_view.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QClipboard>
#include <QLabel>
#include <QLineEdit>
#include <QListView>
#include <QMenu>
#include <QStringListModel>
#include <QVBoxLayout>

CommandHistoryView::CommandHistoryView(MainWindow *window) : QWidget(window), window_(window) {
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    empty_ = new QLabel(i18n::tr("没有可用的历史命令"), this);
    empty_->setAlignment(Qt::AlignCenter);
    empty_->setObjectName(QStringLiteral("mutedHint"));
    model_ = new QStringListModel(this);
    list_ = new QListView(this);
    list_->setObjectName(QStringLiteral("historyList"));
    list_->setModel(model_);
    list_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    list_->setUniformItemSizes(true);
    list_->setTextElideMode(Qt::ElideRight);
    list_->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    layout->addWidget(empty_);
    layout->addWidget(list_, 1);
    connect(list_, &QListView::activated, this, [this](const QModelIndex &index) {
        if (auto *bar = window_->page_sendbar(window_->current_page())) {
            bar->input()->setText(index.data().toString());
            bar->input()->setFocus();
        }
    });
    list_->setContextMenuPolicy(Qt::CustomContextMenu);
    connect(list_, &QListView::customContextMenuRequested, this, [this](const QPoint &pos) {
        const QString command = list_->indexAt(pos).data().toString();
        QMenu menu(this); chrome::style_menu(&menu);
        auto *insert = menu.addAction(i18n::tr("填入当前终端")); insert->setEnabled(!command.isEmpty());
        auto *copy = menu.addAction(i18n::tr("复制")); copy->setEnabled(!command.isEmpty());
        menu.addSeparator();
        auto *clear = menu.addAction(i18n::tr("清空历史命令")); clear->setEnabled(tab_ != UINT32_MAX);
        const auto *picked = menu.exec(list_->viewport()->mapToGlobal(pos));
        if (!picked) return;
        if (picked == copy) host_clipboard::copy_text(command);
        else if (picked == insert) {
            if (auto *pane = window_->active_view()) {
                const QByteArray bytes = command.toUtf8();
                term_write(pane->tab(), rust::Slice<const uint8_t>(
                    reinterpret_cast<const uint8_t *>(bytes.constData()), size_t(bytes.size())));
                pane->setFocus();
            }
        } else if (picked == clear && tab_ != UINT32_MAX) { history_clear(tab_); refresh(tab_); }
    });
}

void CommandHistoryView::refresh(uint32_t tab) {
    const uint64_t revision = tab == UINT32_MAX ? 0 : history_revision(tab);
    if (tab == tab_ && revision == revision_) return;
    tab_ = tab; revision_ = revision; commands_.clear();
    if (tab != UINT32_MAX) for (const auto &command : history_commands(tab))
        commands_ << QString::fromUtf8(command.data(), int(command.size()));
    apply_filter();
}

void CommandHistoryView::set_filter(const QString &text) { filter_ = text; apply_filter(); }

void CommandHistoryView::apply_filter() {
    const QString selected = list_->currentIndex().data().toString();
    QStringList rows;
    for (const auto &command : commands_)
        if (command.contains(filter_, Qt::CaseInsensitive)) rows << command;
    if (model_->stringList() != rows) model_->setStringList(rows);
    const int at = rows.indexOf(selected);
    if (at >= 0) list_->setCurrentIndex(model_->index(at));
    empty_->setVisible(rows.isEmpty()); list_->setVisible(!rows.isEmpty());
}
