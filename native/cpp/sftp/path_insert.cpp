#include "cpp/sftp/sftp_view.h"
#include "cpp/clipboard.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QDialog>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QVBoxLayout>
#include <exception>

void SftpBrowser::insert_path(uint32_t tab, const QString &path) {
    if (context_tab_ != tab) return;
    const auto source = completion_context(tab);
    if (!source.ready) { if (on_note) on_note(i18n::tr("当前会话不可写，请退出阅读或锁定后重试")); return; }
    QDialog dialog(this);
    dialog.setWindowTitle(i18n::tr("将路径填入终端"));
    dialog.resize(560, 260);
    auto *layout = new QVBoxLayout(&dialog);
    auto *hint = new QLabel(i18n::tr("选择目标 Shell 的参数语法。预览只作为字面路径填入当前会话，不追加换行；执行需另行确认。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *syntax = new QComboBox(&dialog);
    syntax->addItems({QStringLiteral("POSIX / Bash"), QStringLiteral("PowerShell")});
    syntax->setCurrentIndex(-1); layout->addWidget(syntax);
    auto *preview = new QPlainTextEdit(&dialog); preview->setReadOnly(true); layout->addWidget(preview, 1);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    buttons->button(QDialogButtonBox::Ok)->setText(i18n::tr("填入终端"));
    buttons->button(QDialogButtonBox::Ok)->setEnabled(false); layout->addWidget(buttons);
    bool valid = false;
    QObject::connect(syntax, &QComboBox::currentIndexChanged, &dialog, [&] {
        try {
            const auto value = files_quote_path(rust::String(path.toUtf8().toStdString()), uint8_t(syntax->currentIndex() + 1));
            preview->setPlainText(QString::fromUtf8(value.data(), int(value.size()))); valid = true;
        } catch (const std::exception &e) { preview->setPlainText(QString::fromUtf8(e.what())); valid = false; }
        buttons->button(QDialogButtonBox::Ok)->setEnabled(valid);
    });
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    if (dialog.exec() != QDialog::Accepted || !valid) return;
    if (context_tab_ != tab || !completion_valid(source)) { if (on_note) on_note(i18n::tr("来源会话已变化，请重新选择路径")); return; }
    if (host_clipboard::copy_text(preview->toPlainText())) term_paste_from_clipboard(tab);
}
