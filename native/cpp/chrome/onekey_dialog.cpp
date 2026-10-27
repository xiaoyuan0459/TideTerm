// OneKey account manager. Secrets stay in Rust memory and never cross back to Qt.
#include "cpp/chrome/onekey_dialog.h"
#include "cpp/i18n.h"
#include "cpp/bridge_shim.h"

#include <QComboBox>
#include <QDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QListWidgetItem>
#include <QMessageBox>
#include <QPushButton>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String rs(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
}

void run_onekey_dialog(QWidget *parent) {
    QDialog dlg(parent);
    dlg.setWindowTitle(i18n::tr("OneKeys 账户"));
    dlg.resize(610, 400);
    auto *root = new QHBoxLayout(&dlg);
    auto *accounts = new QListWidget(&dlg);
    accounts->setMinimumWidth(180);
    root->addWidget(accounts, 1);

    auto *editor = new QWidget(&dlg);
    auto *form = new QFormLayout(editor);
    auto *name = new QLineEdit(editor);
    auto *username = new QLineEdit(editor);
    auto *auth = new QComboBox(editor);
    auth->addItems({i18n::tr("密码"), i18n::tr("私钥")});
    auto *secret = new QLineEdit(editor);
    secret->setEchoMode(QLineEdit::Password);
    secret->setPlaceholderText(i18n::tr("已有账户留空可保留当前凭据"));
    auto *key_path = new QLineEdit(editor);
    form->addRow(i18n::tr("账户名"), name);
    form->addRow(i18n::tr("用户名"), username);
    form->addRow(i18n::tr("认证方式"), auth);
    form->addRow(i18n::tr("密码 / 密钥口令"), secret);
    form->addRow(i18n::tr("私钥路径"), key_path);
    auto *actions = new QWidget(editor);
    auto *row = new QHBoxLayout(actions);
    row->setContentsMargins(0, 0, 0, 0);
    auto *create = new QPushButton(i18n::tr("新建"), actions);
    auto *save = new QPushButton(i18n::tr("保存"), actions);
    auto *remove = new QPushButton(i18n::tr("删除"), actions);
    auto *close = new QPushButton(i18n::tr("关闭"), actions);
    row->addWidget(create);
    row->addWidget(save);
    row->addWidget(remove);
    row->addStretch();
    row->addWidget(close);
    form->addRow(actions);
    root->addWidget(editor, 2);

    QString selected_id;
    auto refresh = [&] {
        accounts->clear();
        for (const OneKeyRow &account : onekey_rows()) {
            auto *item = new QListWidgetItem(q(account.name) + QStringLiteral(" · ")
                + q(account.username), accounts);
            item->setData(Qt::UserRole, q(account.id));
            item->setToolTip(account.kind == 1 ? i18n::tr("私钥认证")
                : i18n::tr("密码认证"));
        }
    };
    QObject::connect(accounts, &QListWidget::currentItemChanged, &dlg,
        [&](QListWidgetItem *item) {
            selected_id = item ? item->data(Qt::UserRole).toString() : QString();
            if (!item) return;
            for (const OneKeyRow &account : onekey_rows()) {
                if (q(account.id) != selected_id) continue;
                name->setText(q(account.name));
                username->setText(q(account.username));
                auth->setCurrentIndex(account.kind == 1 ? 1 : 0);
                key_path->setText(q(account.key_path));
                secret->clear();
                break;
            }
        });
    QObject::connect(create, &QPushButton::clicked, &dlg, [&] {
        accounts->clearSelection();
        accounts->setCurrentItem(nullptr);
        selected_id.clear();
        name->clear(); username->clear(); secret->clear(); key_path->clear();
        auth->setCurrentIndex(0); name->setFocus();
    });
    QObject::connect(save, &QPushButton::clicked, &dlg, [&] {
        const rust::String id = onekey_save(rs(selected_id), rs(name->text()),
            rs(username->text()), uint8_t(auth->currentIndex()),
            rs(secret->text()), rs(key_path->text()));
        if (id.empty()) {
            QMessageBox::warning(&dlg, QStringLiteral("OneKeys"),
                i18n::tr("请填写账户名、用户名和有效凭据"));
            return;
        }
        selected_id = q(id);
        secret->clear();
        refresh();
        for (int i = 0; i < accounts->count(); ++i) {
            if (accounts->item(i)->data(Qt::UserRole).toString() == selected_id) {
                accounts->setCurrentRow(i); break;
            }
        }
    });
    QObject::connect(remove, &QPushButton::clicked, &dlg, [&] {
        if (selected_id.isEmpty()) return;
        if (QMessageBox::question(&dlg, i18n::tr("删除 OneKey 账户"),
            i18n::tr("删除 %1？").arg(name->text())) != QMessageBox::Yes) return;
        onekey_delete(rs(selected_id));
        selected_id.clear();
        refresh();
        name->clear(); username->clear(); secret->clear(); key_path->clear();
    });
    QObject::connect(close, &QPushButton::clicked, &dlg, &QDialog::accept);
    refresh();
    dlg.exec();
}
