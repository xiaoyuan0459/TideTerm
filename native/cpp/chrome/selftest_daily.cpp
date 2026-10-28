#include "cpp/chrome/selftest_daily.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QAction>
#include <QAbstractButton>
#include <QApplication>
#include <QDialog>
#include <QDialogButtonBox>
#include <QComboBox>
#include <QLineEdit>
#include <QPushButton>
#include <QDir>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFileInfo>
#include <QFile>
#include <QTemporaryDir>
#include <QInputDialog>
#include <QKeyEvent>
#include <QMenu>
#include <QMessageBox>
#include <QTimer>
#include <QTreeWidget>
#include <cstdio>

namespace {
bool wait_for(const std::function<bool()> &condition) {
    QElapsedTimer deadline; deadline.start();
    while (!condition() && deadline.elapsed() < 5000) {
        QEventLoop turn;
        QTimer::singleShot(10, &turn, &QEventLoop::quit); turn.exec();
    }
    return condition();
}
bool menu(SftpBrowser *browser, const QString &label, const QString &entry,
          const QStringList &responses, bool cancel = false) {
    auto *tree = browser->local_tree();
    QPoint point(tree->viewport()->width() / 2, tree->viewport()->height() - 2);
    if (!entry.isEmpty()) {
        bool found = false;
        for (int i = 0; i < tree->topLevelItemCount(); ++i) {
            auto *row = tree->topLevelItem(i);
            if (row->data(0, Qt::UserRole).toString() == entry) {
                tree->scrollToItem(row); point = tree->visualItemRect(row).center(); found = true; break;
            }
        }
        if (!found) return false;
    }
    bool picked = false;
    int response = 0;
    QElapsedTimer deadline; deadline.start();
    QTimer driver;
    QObject::connect(&driver, &QTimer::timeout, [&] {
        if (deadline.elapsed() > 2000) {
            if (auto *popup = QApplication::activePopupWidget()) popup->close();
            if (auto *dialog = dynamic_cast<QDialog *>(QApplication::activeModalWidget())) dialog->reject();
            return;
        }
        if (!picked) if (auto *popup = dynamic_cast<QMenu *>(QApplication::activePopupWidget())) {
            for (auto *action : popup->actions()) if (action->text() == label && action->isEnabled()) {
                picked = true; popup->setActiveAction(action);
                QApplication::postEvent(popup, new QKeyEvent(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier));
                return;
            }
        }
        if (auto *input = dynamic_cast<QInputDialog *>(QApplication::activeModalWidget())) {
            if (response < responses.size()) {
                const auto value = responses[response++];
                if (cancel) input->reject();
                else { if (!value.isEmpty()) input->setTextValue(value); input->accept(); }
            }
        } else if (auto *box = dynamic_cast<QMessageBox *>(QApplication::activeModalWidget())) {
            if (auto *yes = box->button(QMessageBox::Yes)) yes->click();
        }
    });
    driver.start(10);
    tree->customContextMenuRequested(point);
    std::printf("[daily] menu %s picked=%d responses=%d/%d rows=%d viewport=%dx%d\n", label.toUtf8().constData(), int(picked), response, int(responses.size()), browser->local_rows(), tree->viewport()->width(), tree->viewport()->height());
    return picked && response == responses.size();
}
bool log_actions(MainWindow *window) {
    const auto tab = window->active_view()->tab();
    auto trigger = [&](const char *id) {
        window->refresh_chrome();
        for (auto *action : window->findChildren<QAction *>()) {
            if (action->property("tid").toByteArray() == id) { action->trigger(); return true; }
        }
        return false;
    };
    QTemporaryDir destination;
    if (!destination.isValid()) return false;
    bool configured = false;
    QTimer setup;
    QObject::connect(&setup, &QTimer::timeout, [&] {
        auto *dialog = QApplication::activeModalWidget();
        if (!dialog || dialog->objectName() != QStringLiteral("logSettings")) return;
        dialog->findChild<QLineEdit *>(QStringLiteral("logDirectory"))->setText(destination.path());
        dialog->findChild<QLineEdit *>(QStringLiteral("logPrefix"))->setText(QStringLiteral("daily"));
        dialog->findChild<QComboBox *>(QStringLiteral("logFormat"))->setCurrentIndex(1);
        configured = true; setup.stop();
        dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Save)->click();
    });
    setup.start(10);
    bool ok = trigger("session:log-settings") && configured;
    const auto saved = slog_settings();
    ok &= QString::fromUtf8(saved.directory.data(), int(saved.directory.size())) == destination.path() && saved.format == 1;
    ok &= trigger("session:log-start") && wait_for([&] { return slog_status(tab).state == 2; });
    const QByteArray output("echo DAILY_LOG_VISIBLE\r");
    term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(output.data()), size_t(output.size())));
    ok &= wait_for([&] { term_poll(tab); const auto text = slog_recent(tab, 100); return QByteArray(text.data(), int(text.size())).contains("DAILY_LOG_VISIBLE"); });
    ok &= trigger("session:log-pause") && slog_status(tab).state == 3;
    ok &= trigger("session:log-resume") && slog_status(tab).state == 2;
    ok &= trigger("session:log-stop") && wait_for([&] { return slog_status(tab).state == 5; });
    const auto path = slog_status(tab).path;
    QFile file(QDir(QString::fromUtf8(path.data(), int(path.size()))).filePath(QStringLiteral("output.log")));
    ok &= file.open(QIODevice::ReadOnly);
    const auto content = file.readAll();
    ok &= content.contains("DAILY_LOG_VISIBLE") && content.startsWith("\n[") && slog_status(tab).written == uint64_t(content.size());
    return ok;
}
}

bool run_daily_ui(MainWindow *window) {
    if (!window->active_view()) return false;
    QTemporaryDir fixture;
    if (!fixture.isValid()) return false;
    const QString directory = fixture.path();
    QFile seed(QDir(directory).filePath(QStringLiteral("fixture.txt")));
    if (!seed.open(QIODevice::WriteOnly)) return false;
    seed.write("fixture"); seed.close();
    // A separate real browser keeps automatic terminal-CWD following from
    // changing this file-menu fixture or C5's asynchronous attach-error probe.
    SftpBrowser standalone;
    auto *browser = &standalone;
    browser->resize(800, 480); browser->show(); browser->scan_local(directory);
    if (!wait_for([&] { browser->poll(); return browser->local_rows() == 1; })) return false;
    const int original = browser->local_rows();
    QApplication::processEvents();
    const auto created = QDir(directory).filePath(QStringLiteral("daily.txt"));
    const auto copied = QDir(directory).filePath(QStringLiteral("daily-copy.txt"));
    bool ok = menu(browser, i18n::tr("新建文件…(F)"), QString(), {QStringLiteral("daily.txt")});
    ok &= wait_for([&] { browser->poll(); return QFileInfo::exists(created) && browser->local_rows() == original + 1; });
    ok &= menu(browser, i18n::tr("复制到…"), QStringLiteral("daily.txt"), {copied, QString()});
    ok &= wait_for([&] { browser->poll(); return QFileInfo::exists(copied) && browser->local_rows() == original + 2; });
    for (const auto &name : {QStringLiteral("daily.txt"), QStringLiteral("daily-copy.txt")}) {
        ok &= menu(browser, i18n::tr("删除(R)"), name, {});
        ok &= wait_for([&] { browser->poll(); return !QFileInfo::exists(QDir(directory).filePath(name)); });
        ok &= wait_for([&] { browser->poll(); return browser->local_rows() == original + (name == QLatin1String("daily.txt") ? 1 : 0); });
    }
    ok &= menu(browser, i18n::tr("新建文件…(F)"), QString(), {QStringLiteral("cancelled.txt")}, true);
    ok &= !QFileInfo::exists(QDir(directory).filePath(QStringLiteral("cancelled.txt")));
    const auto next = QDir(directory).filePath(QStringLiteral("next.txt"));
    QFile next_file(next);
    if (!next_file.open(QIODevice::WriteOnly)) return false;
    next_file.write("next-data"); next_file.close();
    const auto batch = QDir(directory).filePath(QStringLiteral("batch"));
    QDir().mkpath(batch); browser->scan_local(directory);
    ok &= wait_for([&] { browser->poll(); return browser->local_rows() == 3; });
    const auto select_pair = [&] {
        for (int i = 0; i < browser->local_tree()->topLevelItemCount(); ++i) {
            auto *row = browser->local_tree()->topLevelItem(i);
            row->setSelected(row->text(0) == QLatin1String("fixture.txt") || row->text(0) == QLatin1String("next.txt"));
        }
    };
    select_pair();
    bool batch_ok = menu(browser, i18n::tr("复制到…"), QStringLiteral("next.txt"), {batch, QString()});
    batch_ok &= wait_for([&] {
        QFile first(QDir(batch).filePath(QStringLiteral("fixture.txt"))), second(QDir(batch).filePath(QStringLiteral("next.txt")));
        return first.open(QIODevice::ReadOnly) && second.open(QIODevice::ReadOnly) && first.readAll() == "fixture" && second.readAll() == "next-data";
    });
    batch_ok &= wait_for([&] { browser->poll(); return browser->local_rows() == 3; });
    const auto moved = QDir(batch).filePath(QStringLiteral("moved"));
    QDir().mkpath(moved);
    select_pair();
    batch_ok &= menu(browser, i18n::tr("移动到…(M)"), QStringLiteral("next.txt"), {moved}, true);
    batch_ok &= QFileInfo::exists(next) && QFileInfo::exists(seed.fileName());
    batch_ok &= wait_for([&] { browser->poll(); return browser->local_rows() == 3; });
    select_pair();
    batch_ok &= menu(browser, i18n::tr("移动到…(M)"), QStringLiteral("next.txt"), {moved});
    const bool moved_pair = wait_for([&] {
        browser->poll();
        QFile first(QDir(moved).filePath(QStringLiteral("fixture.txt"))), second(QDir(moved).filePath(QStringLiteral("next.txt")));
        return !QFileInfo::exists(next) && !QFileInfo::exists(seed.fileName())
            && first.open(QIODevice::ReadOnly) && second.open(QIODevice::ReadOnly)
            && first.readAll() == "fixture" && second.readAll() == "next-data";
    });
    std::printf("[daily] multi-selection move preserves both file names and bytes: %s\n", moved_pair ? "PASS" : "FAIL");
    if (!moved_pair) return false;
    browser->scan_local(moved);
    batch_ok &= wait_for([&] { browser->poll(); return browser->local_rows() == 2; });
    select_pair();
    batch_ok &= menu(browser, i18n::tr("删除(R)"), QStringLiteral("next.txt"), {});
    batch_ok &= wait_for([&] { browser->poll(); return browser->local_rows() == 0 && !QFileInfo::exists(QDir(moved).filePath(QStringLiteral("next.txt"))) && !QFileInfo::exists(QDir(moved).filePath(QStringLiteral("fixture.txt"))); });
    std::printf("[daily] multi-selection context copy/move/delete and independent file bytes: %s\n", batch_ok ? "PASS" : "FAIL");
    ok &= batch_ok;
    std::printf("[daily] actual local file menus/create/copy/delete/cancel: %s\n", ok ? "PASS" : "FAIL");
    const bool logging = log_actions(window);
    std::printf("[daily] actual log menus/start/pause/resume/stop/file: %s\n", logging ? "PASS" : "FAIL");
    return ok && logging;
}
