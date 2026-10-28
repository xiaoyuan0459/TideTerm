#include "cpp/chrome/log_actions.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/bridge_shim.h"
#include <QAction>
#include <QComboBox>
#include <QDesktopServices>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QUrl>
#include <QVBoxLayout>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
rust::String r(const QString &s) { return rust::String(s.toUtf8().toStdString()); }
}

void chrome::show_log_settings(MainWindow *window) {
    try {
        QString read_error;
        const auto options = [&] {
            try { return slog_settings(); }
            catch (const std::exception &e) { read_error = QString::fromUtf8(e.what()); return slog_default_settings(); }
        }();
        QDialog dialog(window);
        dialog.setObjectName(QStringLiteral("logSettings"));
        dialog.setWindowTitle(i18n::tr("日志设置"));
        dialog.setMinimumWidth(520);
        auto *layout = new QVBoxLayout(&dialog);
        auto *form = new QFormLayout;
        auto *directory_row = new QWidget(&dialog);
        auto *row = new QHBoxLayout(directory_row); row->setContentsMargins(0, 0, 0, 0);
        auto *directory = new QLineEdit(q(options.directory), directory_row);
        directory->setObjectName(QStringLiteral("logDirectory"));
        auto *browse = new QPushButton(i18n::tr("浏览…"), directory_row);
        row->addWidget(directory, 1); row->addWidget(browse);
        auto *prefix = new QLineEdit(q(options.prefix), &dialog);
        prefix->setObjectName(QStringLiteral("logPrefix")); prefix->setMaxLength(48);
        auto *format = new QComboBox(&dialog);
        format->setObjectName(QStringLiteral("logFormat"));
        format->addItems({i18n::tr("原始输出（保留控制序列）"), i18n::tr("带时间戳的输出片段（保留控制序列）")});
        format->setCurrentIndex(options.format);
        auto *size = new QSpinBox(&dialog); size->setObjectName(QStringLiteral("logRotateMiB"));
        size->setRange(1, 256); size->setSuffix(QStringLiteral(" MiB")); size->setValue(int(options.rotate_mib));
        auto *keep = new QSpinBox(&dialog); keep->setObjectName(QStringLiteral("logKeep"));
        keep->setRange(2, 20); keep->setValue(int(options.keep));
        form->addRow(i18n::tr("日志目录"), directory_row);
        form->addRow(i18n::tr("文件名前缀"), prefix);
        form->addRow(i18n::tr("格式"), format);
        form->addRow(i18n::tr("每份大小"), size);
        form->addRow(i18n::tr("每次记录保留份数"), keep);
        layout->addLayout(form);
        auto *hint = new QLabel(i18n::tr("设置保存到本机，仅影响下次开始的记录。名称自动追加时间和唯一编号。轮换上限针对每次记录；历史记录不会自动删除，总磁盘占用可能继续增长。暂停期间不捕获输出。"), &dialog);
        hint->setWordWrap(true); layout->addWidget(hint);
        auto *error = new QLabel(read_error, &dialog); error->setWordWrap(true); error->setTextFormat(Qt::PlainText); layout->addWidget(error);
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel, &dialog);
        layout->addWidget(buttons);
        QObject::connect(browse, &QPushButton::clicked, &dialog, [&] {
            const auto path = QFileDialog::getExistingDirectory(&dialog, i18n::tr("日志目录"), directory->text());
            if (!path.isEmpty()) directory->setText(path);
        });
        QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
        QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] {
            try {
                slog_save_settings(LogSettings{r(directory->text()), r(prefix->text()), uint8_t(format->currentIndex()), uint32_t(size->value()), uint32_t(keep->value())});
                dialog.accept(); window->note(i18n::tr("日志设置已保存，下次开始记录时生效"));
            } catch (const std::exception &e) { error->setText(QString::fromUtf8(e.what())); }
        });
        dialog.exec();
    } catch (const std::exception &e) { window->note(QString::fromUtf8(e.what())); }
}

bool chrome::dispatch_log(MainWindow *window, QAction *action) {
    const auto id = action->property("tid").toByteArray();
    if (id == "session:log-settings") { show_log_settings(window); return true; }
    if (!id.startsWith("session:log-")) return false;
    auto *view = window->active_view();
    if (!view || view->is_closed()) { window->note(i18n::tr("请先打开会话")); return true; }
    const auto tab = view->tab();
    if (id == "session:log-start") {
        slog_start(tab);
        const auto state = slog_status(tab);
        window->note(state.state == 6 ? q(state.error) : state.state == 4 ? i18n::tr("日志仍在排空，请稍后重试") : i18n::tr("正在创建会话日志"));
    } else if (id == "session:log-stop") {
        slog_stop(tab); window->note(i18n::tr("已停止捕获，正在排空日志"));
    } else if (id == "session:log-pause" || id == "session:log-resume") {
        slog_pause(tab, id == "session:log-pause");
    } else if (id == "session:log-open") {
        const auto state = slog_status(tab);
        if (state.path.empty()) window->note(i18n::tr("当前会话尚未创建持续日志"));
        else if (!QDesktopServices::openUrl(QUrl::fromLocalFile(q(state.path)))) window->note(i18n::tr("无法打开日志目录"));
    }
    window->refresh_chrome();
    return true;
}

void chrome::refresh_log_actions(MainWindow *window) {
    auto *view = window->active_view();
    const bool ready = view && !view->is_closed();
    const auto state = ready ? slog_status(view->tab()) : LogStatus{};
    const struct { const char *id; bool enabled; } rows[] = {
        {"session:log-start", ready && (state.state == 0 || state.state >= 5)},
        {"session:log-stop", ready && state.state > 0 && state.state < 4},
        {"session:log-pause", ready && state.state == 2},
        {"session:log-resume", ready && state.state == 3},
        {"session:log-open", ready && !state.path.empty()},
    };
    for (const auto &row : rows) if (auto *action = find_menu_action(window->menuBar(), QString::fromLatin1(row.id))) action->setEnabled(row.enabled);
}
