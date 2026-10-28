#include "cpp/chrome/send_completion.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QDialog>
#include <QCheckBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>
#include <algorithm>
#include <vector>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String r(const QString &s) { const auto b = s.toUtf8(); return rust::String(b.constData(), size_t(b.size())); }
QString error_text(uint8_t error) {
    switch (error) {
    case 1: return i18n::tr("用户命令文件无法读取或格式无效；修复文件后重启应用。");
    case 2: return i18n::tr("名称、单行命令或模板无效：最多 200 项，名称/分组 128 字节，命令 4096 字节；模板需有效参数。");
    case 3: return i18n::tr("用户命令已被其他窗口修改；请保留草稿，重新打开后合并。");
    case 4: return i18n::tr("用户命令保存失败；原文件和编辑草稿已保留。");
    case 5: return i18n::tr("应用已锁定，不能保存用户命令。");
    default: return {};
    }
}
}
namespace chrome {
void edit_user_commands(QWidget *owner) {
    auto catalog = command_catalog();
    std::vector<UserCommand> entries;
    for (const auto &e : catalog.entries) entries.push_back(e);
    QDialog dialog(owner);
    dialog.setObjectName(QStringLiteral("userCommandsDialog"));
    dialog.setWindowTitle(i18n::tr("用户命令")); dialog.resize(700, 420);
    auto *layout = new QVBoxLayout(&dialog);
    auto *hint = new QLabel(i18n::tr("仅显式保存用户命令；历史不会写入此目录。选用命令后请检查目标环境，再发送。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *body = new QHBoxLayout;
    auto *list = new QListWidget(&dialog); list->setObjectName(QStringLiteral("userCommandsList"));
    list->setMinimumWidth(180); body->addWidget(list, 1);
    auto *form = new QFormLayout;
    auto entry = [&dialog, form](const char *name, const QString &label, int max) {
        auto *edit = new QLineEdit(&dialog); edit->setObjectName(QString::fromLatin1(name));
        edit->setMaxLength(max); form->addRow(label, edit); return edit;
    };
    auto *group = entry("userCommandGroup", i18n::tr("分组"), 128);
    auto *name = entry("userCommandName", i18n::tr("名称"), 128);
    auto *command = entry("userCommandText", i18n::tr("单行命令"), 4096);
    auto *template_mode = new QCheckBox(i18n::tr("启用参数模板（{{name}}）"), &dialog);
    template_mode->setObjectName(QStringLiteral("userCommandTemplate")); form->addRow(template_mode);
    template_mode->setToolTip(i18n::tr("模板需至少一个参数，参数名使用字母/数字/下划线，不能以数字开头；保存时不展开。"));
    body->addLayout(form, 2); layout->addLayout(body, 1);
    auto *actions = new QHBoxLayout;
    auto *add = new QPushButton(i18n::tr("新增"), &dialog); add->setObjectName(QStringLiteral("userCommandAdd"));
    auto *remove = new QPushButton(i18n::tr("删除"), &dialog); remove->setObjectName(QStringLiteral("userCommandDelete"));
    actions->addWidget(add); actions->addWidget(remove); actions->addStretch(); layout->addLayout(actions);
    auto *error = new QLabel(error_text(catalog.error), &dialog);
    error->setObjectName(QStringLiteral("userCommandError")); error->setWordWrap(true); layout->addWidget(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel, &dialog);
    buttons->button(QDialogButtonBox::Save)->setText(i18n::tr("保存"));
    buttons->button(QDialogButtonBox::Cancel)->setText(i18n::tr("取消"));
    buttons->button(QDialogButtonBox::Save)->setObjectName(QStringLiteral("userCommandSave"));
    // Enter in the editor must never implicitly commit a catalog or send a command.
    for (auto *button : dialog.findChildren<QPushButton *>()) { button->setAutoDefault(false); button->setDefault(false); }
    layout->addWidget(buttons);
    auto refresh_label = [&] {
        const int row = list->currentRow();
        if (row < 0) return;
        list->item(row)->setText(group->text().isEmpty() ? name->text() : group->text() + QStringLiteral(" / ") + name->text());
    };
    auto update = [&] {
        const int row = list->currentRow();
        if (row < 0) return;
        auto &e = entries[size_t(row)]; e.group = r(group->text()); e.name = r(name->text()); e.command = r(command->text());
        e.is_template = template_mode->isChecked();
        refresh_label();
    };
    auto select = [&](int row) {
        const bool selected = row >= 0;
        const QSignalBlocker a(group), b(name), c(command), d(template_mode);
        group->setEnabled(selected); name->setEnabled(selected); command->setEnabled(selected); remove->setEnabled(selected);
        group->setText(selected ? q(entries[size_t(row)].group) : QString());
        name->setText(selected ? q(entries[size_t(row)].name) : QString());
        command->setText(selected ? q(entries[size_t(row)].command) : QString());
        template_mode->setEnabled(selected); template_mode->setChecked(selected && entries[size_t(row)].is_template);
    };
    QObject::connect(list, &QListWidget::currentRowChanged, &dialog, select);
    for (auto *edit : {group, name, command}) QObject::connect(edit, &QLineEdit::textEdited, &dialog, update);
    QObject::connect(template_mode, &QCheckBox::toggled, &dialog, update);
    QObject::connect(add, &QPushButton::clicked, &dialog, [&] {
        uint32_t id = 1;
        while (std::any_of(entries.begin(), entries.end(), [id](const auto &e) { return e.id == id; })) ++id;
        if (entries.size() >= 200) { error->setText(error_text(2)); return; }
        UserCommand e; e.id = id; entries.push_back(std::move(e));
        list->addItem(i18n::tr("新命令")); list->setCurrentRow(list->count() - 1); name->setFocus();
    });
    QObject::connect(remove, &QPushButton::clicked, &dialog, [&] {
        const int row = list->currentRow(); if (row < 0) return;
        // Suppress row signals until the POD vector and list have the same indices again.
        { const QSignalBlocker block(list);
          entries.erase(entries.begin() + row); delete list->takeItem(row); }
        list->setCurrentRow(std::min(row, list->count() - 1));
        select(list->currentRow());
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] {
        const uint8_t result = command_catalog_save(catalog.revision,
            rust::Slice<const UserCommand>(entries.data(), entries.size()));
        if (result == 0) dialog.accept(); else error->setText(error_text(result));
    });
    for (const auto &e : catalog.entries) list->addItem(e.group.empty() ? q(e.name) : q(e.group) + QStringLiteral(" / ") + q(e.name));
    list->setCurrentRow(-1); select(-1);
    if (list->count()) list->setCurrentRow(0);
    buttons->button(QDialogButtonBox::Save)->setEnabled(catalog.error == 0);
    add->setEnabled(catalog.error == 0);
    dialog.exec();
}
}
