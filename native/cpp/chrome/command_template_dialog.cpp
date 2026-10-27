#include "cpp/chrome/send_completion.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPointer>
#include <QScrollArea>
#include <QPushButton>
#include <QTimer>
#include <QToolTip>
#include <QVBoxLayout>
#include <vector>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
QString error_text(uint8_t error) {
    if (error == 1 || error == 2) return i18n::tr("命令或来源已失效，请重新选择。");
    if (error == 3) return i18n::tr("模板格式无效：使用 {{name}}，最多 16 个不同参数。");
    return i18n::tr("请填写全部参数；仅支持单行，展开结果最多 4096 字节。");
}
}
namespace chrome {
bool fill_user_command(QWidget *owner, QLineEdit *input, const CompletionContext &context, uint32_t id,
                       const std::function<bool()> &valid) {
    auto catalog = command_catalog();
    const UserCommand *entry = nullptr;
    for (const auto &e : catalog.entries) if (e.id == id) entry = &e;
    if (!entry || !valid() || !completion_valid(context)) {
        QToolTip::showText(input->mapToGlobal(QPoint(0, 0)), error_text(1), input); return false;
    }
    if (!entry->is_template) {
        const auto result = command_template_render(context, id, {});
        if (result.error) return false;
        input->setText(q(result.text)); input->setFocus(); return true;
    }
    const auto fields = command_template_fields(entry->command);
    if (fields.error) { QToolTip::showText(input->mapToGlobal(QPoint(0,0)), error_text(3), input); return false; }
    const QPointer<QLineEdit> draft(input);
    QPointer<QDialog> dialog = new QDialog(owner);
    dialog->setObjectName(QStringLiteral("commandTemplateDialog"));
    dialog->setWindowTitle(i18n::tr("填写命令参数")); dialog->resize(640, 360);
    auto *layout = new QVBoxLayout(dialog.data());
    auto *hint = new QLabel(i18n::tr("参数按原文插入，不自动加引号或展开变量。请在草稿中检查 Shell 语法和目标环境；此处不会发送。"), dialog.data());
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *scroll = new QScrollArea(dialog); scroll->setWidgetResizable(true); scroll->setMaximumHeight(240);
    auto *form_widget = new QWidget(scroll); auto *form = new QFormLayout(form_widget);
    std::vector<QLineEdit *> inputs;
    for (const auto &name : fields.names) {
        auto *edit = new QLineEdit(dialog.data()); edit->setObjectName(QStringLiteral("templateParam_") + q(name));
        edit->setMaxLength(4096); form->addRow(q(name), edit); inputs.push_back(edit);
    }
    scroll->setWidget(form_widget); layout->addWidget(scroll);
    auto *preview = new QPlainTextEdit(dialog.data()); preview->setReadOnly(true);
    preview->setObjectName(QStringLiteral("templatePreview")); layout->addWidget(preview, 1);
    auto *error = new QLabel(dialog.data()); error->setWordWrap(true); error->setObjectName(QStringLiteral("templateError")); layout->addWidget(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dialog.data());
    auto *accept = buttons->button(QDialogButtonBox::Ok); accept->setText(i18n::tr("填入草稿"));
    accept->setObjectName(QStringLiteral("templateApply"));
    buttons->button(QDialogButtonBox::Cancel)->setText(i18n::tr("取消")); layout->addWidget(buttons);
    for (auto *button : buttons->buttons()) if (auto *push = dynamic_cast<QPushButton *>(button)) { push->setAutoDefault(false); push->setDefault(false); }
    auto values = [&] {
        rust::Vec<rust::String> result;
        for (const auto *edit : inputs) { const auto bytes = edit->text().toUtf8(); result.push_back(rust::String(bytes.constData(), size_t(bytes.size()))); }
        return result;
    };
    QObject callbacks;
    auto update = [&] {
        auto input_values = values();
        auto result = command_template_render(context, id, rust::Slice<const rust::String>(input_values.data(), input_values.size()));
        if (!valid()) result.error = 1;
        const QString text = result.error ? QString() : q(result.text);
        if (preview->toPlainText() != text) preview->setPlainText(text);
        error->setText(result.error ? error_text(result.error) : QString()); accept->setEnabled(result.error == 0);
    };
    for (auto *edit : inputs) QObject::connect(edit, &QLineEdit::textChanged, &callbacks, update);
    auto *timer = new QTimer(dialog); timer->setInterval(200); QObject::connect(timer, &QTimer::timeout, &callbacks, [&] {
        if (!valid() || !completion_valid(context)) update();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog.data(), &QDialog::reject);
    QString result_text;
    QObject::connect(buttons, &QDialogButtonBox::accepted, &callbacks, [&] {
        auto input_values = values(); const auto result = command_template_render(context, id,
            rust::Slice<const rust::String>(input_values.data(), input_values.size()));
        if (!valid() || result.error) { update(); return; }
        result_text = q(result.text); dialog->accept();
    });
    update(); timer->start();
    if (!inputs.empty()) inputs.front()->setFocus();
    const int verdict = dialog->exec();
    if (!dialog) return false;
    timer->stop(); dialog->deleteLater();
    if (verdict != QDialog::Accepted || !draft || !valid() || !completion_valid(context)) return false;
    draft->setText(result_text); draft->setFocus(); return true;
}
}
