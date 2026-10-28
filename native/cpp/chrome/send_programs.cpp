#include "cpp/chrome/send_programs.h"
#include "cpp/i18n.h"
#include <QAction>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFontDatabase>
#include <QHBoxLayout>
#include <QLabel>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPointer>
#include <QSignalBlocker>
#include <QToolButton>
#include <QWindow>
#include <QVBoxLayout>

namespace chrome {
ProgramSettings::ProgramSettings(QWidget *owner, QHBoxLayout *row, std::function<uint32_t()> source,
        std::function<void()> invalidate, std::function<void()> refresh,
        std::function<QString(uint8_t)> explain)
    : QObject(owner), owner_(owner), source_(std::move(source)), invalidate_(std::move(invalidate)),
      refresh_(std::move(refresh)), explain_(std::move(explain)) {
    button_ = new QToolButton(owner);
    button_->setObjectName(QStringLiteral("sendProgramDirectories"));
    button_->setText(i18n::tr("程序目录")); button_->setCheckable(true);
    button_->setToolTip(i18n::tr("可选指定程序目录建议：当前会话的终端与发送栏共享设置，只读目录元数据，不保证程序可执行或版本；重连后需重新配置。"));
    button_->setAccessibleName(button_->toolTip());
    button_->setPopupMode(QToolButton::MenuButtonPopup);
    auto *menu = new QMenu(button_);
    auto *configure = menu->addAction(i18n::tr("配置程序目录…"));
    configure->setObjectName(QStringLiteral("sendEditProgramDirectories"));
    button_->setMenu(menu);
    connect(configure, &QAction::triggered, this, [this] {
        const QPointer<ProgramSettings> alive(this); edit(); if (alive) refresh_();
    });
    connect(button_, &QToolButton::clicked, this, [this] {
        const QPointer<ProgramSettings> alive(this);
        const auto context = completion_context(source_());
        if (enabled() && (!settings_.configured || settings_.source != context.source
                || settings_.generation != context.generation)) {
            const bool saved = edit();
            if (!alive) return;
            if (!saved) {
                const QSignalBlocker block(button_); button_->setChecked(false);
            }
        }
        refresh_();
    });
    row->addWidget(button_);
}
bool ProgramSettings::enabled() const { return button_->isChecked(); }
void ProgramSettings::sync(ProgramSource settings, bool enabled) {
    settings_ = std::move(settings);
    const QSignalBlocker block(button_); button_->setChecked(enabled);
}
EnvironmentBatch ProgramSettings::request(uint64_t owner, const CompletionContext &context, rust::Str query, uint8_t dialect) const {
    return programs_request(owner, context, query, dialect, settings_);
}
bool ProgramSettings::edit() {
    invalidate_();
    const QPointer<ProgramSettings> alive(this);
    const QPointer<QWidget> owner(owner_);
    const auto source = source_; const auto explain = explain_;
    const auto origin = completion_context(source_());
    // Keep the stack dialog independent of a sender that may be retired while
    // its nested event loop is running; destruction rejects it immediately.
    QDialog dialog; dialog.setWindowModality(Qt::ApplicationModal);
    dialog.winId();
    if (owner_ && owner_->window()->windowHandle())
        dialog.windowHandle()->setTransientParent(owner_->window()->windowHandle());
    connect(this, &QObject::destroyed, &dialog, &QDialog::reject);
    connect(owner_, &QObject::destroyed, &dialog, &QDialog::reject);
    dialog.setObjectName(QStringLiteral("programDirectoriesDialog"));
    dialog.setWindowTitle(i18n::tr("指定程序目录")); dialog.resize(640, 340);
    auto *layout = new QVBoxLayout(&dialog);
    auto *hint = new QLabel(i18n::tr("每行一个绝对目录，最多 8 个。使用当前会话的本地路径或 SSH SFTP 路径；仅扫描文件元数据，不代表 Shell 的 PATH、别名或版本。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *edit = new QPlainTextEdit(&dialog); edit->setObjectName(QStringLiteral("programDirectoryLines"));
    edit->setAccessibleName(i18n::tr("程序目录列表"));
    edit->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    QStringList lines;
    if (settings_.configured && settings_.source == origin.source && settings_.generation == origin.generation) {
        for (const auto &dir : settings_.directories) lines.append(QString::fromUtf8(dir.data(), int(dir.size())));
    }
    edit->setPlainText(lines.join(QLatin1Char('\n'))); layout->addWidget(edit);
    auto *error = new QLabel(&dialog); error->setObjectName(QStringLiteral("programDirectoriesError"));
    error->setWordWrap(true); layout->addWidget(error);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel, &dialog);
    layout->addWidget(buttons);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] {
        if (!alive || !owner) { dialog.reject(); return; }
        ProgramSource next; next.configured = true; next.source = origin.source; next.generation = origin.generation;
        for (const auto &line : edit->toPlainText().split(QLatin1Char('\n'), Qt::SkipEmptyParts))
            next.directories.push_back(rust::String(line.toUtf8().toStdString()));
        const auto state = programs_validate(completion_context(source()), next);
        if (state) { error->setText(explain(state)); return; }
        settings_ = std::move(next); dialog.accept();
    });
    const bool saved = dialog.exec() == QDialog::Accepted;
    if (alive && owner) { owner->window()->raise(); owner->window()->activateWindow(); }
    return saved;
}
}
