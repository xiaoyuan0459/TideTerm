#include "cpp/chrome/selftest_shell_report.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTemporaryDir>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <QTreeWidget>
#include <cstdio>
#include <functional>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
bool wait_for(const std::function<bool()> &condition) {
    QElapsedTimer time; time.start();
    do { term_pump(); QApplication::processEvents(); if (condition()) return true; QThread::msleep(10); } while (time.elapsed() < 12000);
    return false;
}
void write(uint32_t tab, const QString &text) {
    const auto bytes=text.toUtf8(); term_write(tab, rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()),size_t(bytes.size())));
}
void key(QWidget *widget,int code,Qt::KeyboardModifiers mods=Qt::NoModifier,const QString &text={}) {
    QKeyEvent event(QEvent::KeyPress,code,mods,text); QApplication::sendEvent(widget,&event);
}
}
bool selftest_ui::verify_names(MainWindow &window, SendBar *bar, uint32_t tab, const QString &png) {
    QTemporaryDir fixture; if (!fixture.isValid()) return false;
    const auto marker=fixture.filePath(QStringLiteral("executed.txt"));
#ifdef Q_OS_WIN
    const int dialect=2;
    write(tab,QStringLiteral("function global:sysTideNames { [IO.File]::WriteAllText('")+QString(marker).replace(QLatin1Char('\''),QStringLiteral("''"))
        +QStringLiteral("','ran') }; Set-Alias -Scope Global sysTideAlias sysTideNames; [Console]::WriteLine(('NAMES_'+'READY'))\r"));
#else
    const int dialect=1;
    write(tab,QStringLiteral("sysTideNames() { builtin printf ran > '")+QString(marker).replace(QLatin1Char('\''),QStringLiteral("'\\''"))
        +QStringLiteral("'; }; alias sysTideAlias=sysTideNames; builtin printf 'NAMES_%s\\n' READY\r"));
#endif
    const auto prompt=[&] { const auto s=q(term_screen_text(tab)).trimmed(); return s.endsWith(QLatin1Char('>')) || s.endsWith(QLatin1Char('$')); };
    if (!wait_for([&] { return q(term_screen_text(tab)).contains(QStringLiteral("NAMES_READY")) && prompt(); })) return false;
    auto *input=bar->input(); const auto original=input->text(); const auto sent=bar->sent_count();
    auto *button=bar->findChild<QToolButton *>(QStringLiteral("sendShellReport"));
    auto *popup=bar->findChild<QListWidget *>(QStringLiteral("sendSuggestions"));
    if (!button || !popup) return false;
    bool ok=true;
    QTimer watchdog; watchdog.setSingleShot(true);
    QObject::connect(&watchdog,&QTimer::timeout,&window,[&] { ok=false; if (auto *d=dynamic_cast<QDialog *>(QApplication::activeModalWidget())) d->reject(); });
    watchdog.start(25000);
    QTimer::singleShot(0,&window,[&] {
        auto *d=dynamic_cast<QDialog *>(QApplication::activeModalWidget()); if (!d) { ok=false; return; }
        auto *mode=d->findChild<QComboBox *>(QStringLiteral("shellReportMode"));
        auto *syntax=d->findChild<QComboBox *>(QStringLiteral("shellReportSyntax"));
        auto *prefix=d->findChild<QLineEdit *>(QStringLiteral("shellLookupName"));
        auto *refresh=d->findChild<QPushButton *>(QStringLiteral("shellReportRefresh"));
        auto *confirm=d->findChild<QCheckBox *>(QStringLiteral("shellReportConfirm"));
        auto *send=d->findChild<QPushButton *>(QStringLiteral("shellReportSend"));
        auto *result=d->findChild<QPlainTextEdit *>(QStringLiteral("shellReportPath"));
        if (!mode || !syntax || !prefix || !refresh || !confirm || !send || !result) { ok=false; d->reject(); return; }
        mode->setCurrentIndex(5); syntax->setCurrentIndex(dialect); prefix->setText(QStringLiteral("sysTide")); refresh->click();
        ok &= shell_report_poll(tab).state==1 && !send->isEnabled() && input->text()==original && !QFileInfo::exists(marker);
        confirm->setChecked(true); send->click();
        ok &= wait_for([&] { return shell_report_poll(tab).state==3 && result->toPlainText().contains(QStringLiteral("sysTideNames")) && prompt(); });
        if (!png.isEmpty()) ok &= d->grab().save(png+QStringLiteral(".names-ready.png"));
        d->reject();
    }); button->click(); watchdog.stop();
    if (!activate_window(window)) return false;
    input->setFocus(); input->setText(QStringLiteral("sTN")); key(input,Qt::Key_Space,Qt::ControlModifier);
    int row=-1;
    for (int i=0;i<popup->count();++i) if (popup->item(i)->data(Qt::UserRole+1).toInt()==12 && popup->item(i)->data(Qt::UserRole).toString()==QStringLiteral("sysTideNames")) row=i;
    const bool sender=row>=0 && popup->isVisible(); ok &= sender;
    bool stable=false;
    if (sender) { popup->setCurrentRow(row); bar->sync_completion_source(); stable=popup->isVisible(); ok &= stable; key(input,Qt::Key_Tab); }
    const bool sender_filled=input->text()==QStringLiteral("sysTideNames") && bar->sent_count()==sent && !QFileInfo::exists(marker);
    ok &= sender_filled;
    auto *view=window.active_view(); if (!view || view->tab()!=tab) return false;
    auto *terminal=view->findChild<QTreeWidget *>(QStringLiteral("terminalSuggestions")); if (!terminal) return false;
    view->setFocus(); key(view,0,Qt::NoModifier,QStringLiteral("sTN"));
    const bool shown=wait_for([&] {
        for (int i=0;i<terminal->topLevelItemCount();++i) if (terminal->topLevelItem(i)->text(0)==QStringLiteral("sysTideNames")) { terminal->setCurrentItem(terminal->topLevelItem(i)); return terminal->isVisible(); }
        return false;
    });
    if (shown) key(view,Qt::Key_Tab);
    const bool filled=shown && wait_for([&] { return q(term_screen_text(tab)).trimmed().endsWith(QStringLiteral("sysTideNames")); });
    ok &= filled && !QFileInfo::exists(marker);
    if (!png.isEmpty()) ok &= window.grab().save(png+QStringLiteral(".names-terminal.png"));
    write(tab,QString(QChar(3))); wait_for(prompt);
    // Positive control: the function is executable, but only this later explicit
    // command may create the marker; candidate acceptance above must not do so.
    write(tab,QStringLiteral("sysTideNames\r"));
    const bool executed=wait_for([&] { return QFileInfo::exists(marker) && prompt(); });
    shell_report_clear(tab); input->clear();
    std::printf("[names] all=%d sender-list=%d refresh-stable=%d sender-fill=%d terminal=%d explicit-control=%d\n",ok,sender,stable,sender_filled,filled,executed);
    return ok && executed;
}
