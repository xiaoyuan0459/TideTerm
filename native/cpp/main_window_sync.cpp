// Qt edits a bounded draft; Rust owns window membership and atomic admission.
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/terminal/terminal_view.h"
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGridLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>
#include <array>

namespace {
QString q(const rust::String &value) { return QString::fromUtf8(value.data(), int(value.size())); }
QString error_text(uint8_t error) {
    if (error == 3) return i18n::tr("需要四个不重复的通道名称，每个最多 64 个字符且不能包含控制字符");
    if (error == 4) return i18n::tr("会话成员已关闭或重复，请重新打开同步配置");
    if (error == 6) return i18n::tr("锁定屏幕期间不能更改同步输入");
    return i18n::tr("同步会话数量或窗口归属已变化，请重新打开配置");
}
}

bool MainWindow::update_sync_scope() {
    if (!sync_owner_) return true;
    std::vector<uint32_t> ids;
    for (auto *view : views(this)) if (!view->is_closed()) ids.push_back(view->tab());
    return term_sync_update(sync_owner_, rust::Slice<const uint32_t>(ids.data(), ids.size()));
}

void MainWindow::show_sync_channels() {
    if (!sync_owner_) sync_owner_ = term_sync_create();
    if (!sync_owner_ || !update_sync_scope()) { note(error_text(2)); return; }
    const auto snapshot = term_sync_snapshot(sync_owner_);
    if (snapshot.size() != 4) { note(error_text(7)); return; }
    QDialog dialog(this);
    dialog.setObjectName(QStringLiteral("syncChannels"));
    dialog.setWindowTitle(i18n::tr("同步输入通道")); dialog.resize(660, 560);
    auto *layout = new QVBoxLayout(&dialog);
    auto *hint = new QLabel(i18n::tr("四个通道仅在当前窗口、本次运行中生效。相同名称不会跨窗口同步；每个会话只能加入一个通道。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *names_layout = new QGridLayout;
    std::array<QLineEdit *, 4> names{};
    for (int index = 0; index < 4; ++index) {
        names[size_t(index)] = new QLineEdit(q(snapshot[size_t(index)].name), &dialog);
        names[size_t(index)]->setObjectName(QStringLiteral("syncName%1").arg(index));
        names[size_t(index)]->setMaxLength(64);
        auto *label = new QLabel(i18n::tr("通道 %1").arg(index + 1), &dialog);
        label->setBuddy(names[size_t(index)]);
        names_layout->addWidget(label, index / 2, (index % 2) * 2);
        names_layout->addWidget(names[size_t(index)], index / 2, (index % 2) * 2 + 1);
    }
    layout->addLayout(names_layout);
    auto *table = new QTableWidget(0, 2, &dialog);
    table->setObjectName(QStringLiteral("syncMembers"));
    table->setHorizontalHeaderLabels({i18n::tr("会话"), i18n::tr("同步通道")});
    table->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
    table->horizontalHeader()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    table->verticalHeader()->hide(); table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    for (auto *view : views(this)) {
        if (view->is_closed()) continue;
        const int row = table->rowCount(); table->insertRow(row);
        auto *item = new QTableWidgetItem(i18n::tr("%1 · 会话 %2").arg(q_title(view->tab())).arg(row + 1));
        item->setData(Qt::UserRole, view->tab()); item->setToolTip(q(term_session_url(view->tab())));
        table->setItem(row, 0, item);
        auto *channel = new QComboBox(table);
        channel->setAccessibleName(item->text()); channel->addItem(i18n::tr("不加入同步"));
        for (int index = 0; index < 4; ++index) {
            channel->addItem(names[size_t(index)]->text());
            for (const auto id : snapshot[size_t(index)].members)
                if (id == view->tab()) channel->setCurrentIndex(index + 1);
        }
        table->setCellWidget(row, 1, channel);
    }
    layout->addWidget(table, 1);
    for (int index = 0; index < 4; ++index)
        connect(names[size_t(index)], &QLineEdit::textChanged, &dialog, [table, index](const QString &text) {
            for (int row = 0; row < table->rowCount(); ++row)
                static_cast<QComboBox *>(table->cellWidget(row, 1))->setItemText(index + 1, text);
        });
    auto *status = new QLabel(&dialog); status->setTextFormat(Qt::PlainText); status->setWordWrap(true);
    layout->addWidget(status);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    auto *stop = buttons->addButton(i18n::tr("立即停止全部同步"), QDialogButtonBox::ActionRole);
    stop->setObjectName(QStringLiteral("syncStopAll"));
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    connect(stop, &QPushButton::clicked, &dialog, [&] {
        auto current = term_sync_snapshot(sync_owner_);
        for (auto &channel : current) channel.members.clear();
        const auto error = term_sync_replace(sync_owner_, rust::Slice<const SyncChannel>(current.data(), current.size()));
        if (error) status->setText(error_text(error));
        else { note(i18n::tr("本窗口同步输入已停止")); dialog.accept(); }
    });
    connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] {
        if (!update_sync_scope()) { status->setText(error_text(2)); return; }
        rust::Vec<SyncChannel> next;
        for (int index = 0; index < 4; ++index) {
            SyncChannel channel;
            channel.name = rust::String(names[size_t(index)]->text().toUtf8().toStdString());
            for (int row = 0; row < table->rowCount(); ++row)
                if (static_cast<QComboBox *>(table->cellWidget(row, 1))->currentIndex() == index + 1)
                    channel.members.push_back(table->item(row, 0)->data(Qt::UserRole).toUInt());
            next.push_back(std::move(channel));
        }
        const auto error = term_sync_replace(sync_owner_, rust::Slice<const SyncChannel>(next.data(), next.size()));
        if (error) status->setText(error_text(error));
        else { note(i18n::tr("同步输入通道已更新")); dialog.accept(); }
    });
    layout->addWidget(buttons); dialog.exec(); refresh_chrome();
}
