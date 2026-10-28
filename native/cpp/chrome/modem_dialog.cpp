#include "cpp/chrome/modem_dialog.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/i18n.h"
#include <QDialog>
#include <QDialogButtonBox>
#include <QFileDialog>
#include <QInputDialog>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>
namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
}
namespace chrome {
void run_modem_dialog(MainWindow *window) {
    auto *view = window->active_view();
    if (!view) return;
    const uint32_t tab = view->tab();
    const auto cfg = sess_active_config(tab);
    if (cfg.kind != 1 && cfg.kind != 3 && cfg.kind != 4) {
        window->note(i18n::tr("Modem 传输需要 SSH、串口或 TCP 会话"));
        return;
    }
    bool ok = false;
    const QStringList choices = {i18n::tr("发送 XMODEM"), i18n::tr("接收 XMODEM"),
        i18n::tr("发送 YMODEM"), i18n::tr("接收 YMODEM"), i18n::tr("发送 ZMODEM"), i18n::tr("接收 ZMODEM")};
    const QString choice = QInputDialog::getItem(window, QStringLiteral("X/Y/Z Modem"),
        i18n::tr("传输方式"), choices, 4, false, &ok);
    if (!ok) return;
    const int index = choices.indexOf(choice);
    const bool receive = index % 2 != 0;
    const uint8_t kind = uint8_t(index / 2);
    QStringList paths;
    if (receive) {
        const QString path = kind == 0 ? QFileDialog::getSaveFileName(window, i18n::tr("接收文件"))
            : QFileDialog::getExistingDirectory(window, i18n::tr("接收目录"));
        if (!path.isEmpty()) paths.append(path);
    } else if (kind == 0) {
        const QString path = QFileDialog::getOpenFileName(window, i18n::tr("发送文件"));
        if (!path.isEmpty()) paths.append(path);
    } else { paths = QFileDialog::getOpenFileNames(window, i18n::tr("发送文件")); }
    if (paths.isEmpty()) return;
    rust::Vec<rust::String> values;
    for (const auto &path : paths) values.push_back(rust::String(path.toUtf8().toStdString()));
    const auto error = modem_start(tab, kind, receive, std::move(values));
    if (!error.empty()) { window->note(q(error)); return; }
    QDialog dialog(window);
    dialog.setWindowTitle(choice);
    dialog.resize(460, 180);
    auto *layout = new QVBoxLayout(&dialog);
    auto *label = new QLabel(i18n::tr("等待对端握手…"), &dialog);
    label->setTextFormat(Qt::PlainText);
    label->setWordWrap(true);
    layout->addWidget(label);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Cancel, &dialog);
    layout->addWidget(buttons);
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    QTimer timer;
    QObject::connect(&timer, &QTimer::timeout, &dialog, [&] {
        const auto progress = modem_progress(tab);
        QString text = i18n::tr("已传输 %1 字节").arg(qulonglong(progress.bytes));
        if (!progress.file.empty()) text = q(progress.file) + QLatin1Char('\n') + text;
        if (progress.finished && !progress.busy) {
            text += QLatin1Char('\n') + (progress.error.empty() ? i18n::tr("传输完成") : q(progress.error));
            buttons->button(QDialogButtonBox::Cancel)->setText(i18n::tr("关闭"));
            timer.stop();
        }
        label->setText(text);
    });
    timer.start(100);
    dialog.exec();
    if (modem_progress(tab).busy) modem_cancel(tab);
}
}
