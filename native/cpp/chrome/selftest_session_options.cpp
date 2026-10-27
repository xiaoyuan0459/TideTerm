// Real form -> bridge -> atomic store -> future/edit form option round trips.
#include "cpp/chrome/selftest_session_options.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"
#include <QCheckBox>
#include <QTabBar>
#include <QComboBox>
#include <QLineEdit>
#include <QSpinBox>
#include <QInputMethodEvent>
#include <QThread>
#include <cstdio>

#ifdef Q_OS_WIN
static bool check_pwsh_terminal(QWidget *parent) {
    auto *window = dynamic_cast<MainWindow *>(parent);
    if (!window) return false;
    SessCfg draft = sess_form_get("", 0);
    draft.local_shell = "pwsh";
    draft.local_path = "";
    draft.local_args = "";
    const auto id = sess_save(draft);
    if (id.empty()) return false;
    const auto tab = sess_open(id);
    if (tab == UINT32_MAX) { sess_delete(id); return false; }
    window->add_tab_with(tab); // Use the saved-session product path and its owned pump.
    const int page = window->current_page();
    auto *view = window->pane_view(page, 0);
    view->setFocus();
    const bool focused = selftest_ui::activate_window(*window);
    const auto contains = [&](const char *marker) {
        const auto screen = term_screen_text(tab);
        return std::string(screen.data(), screen.size()).find(marker) != std::string::npos;
    };
    const auto wait_for = [&](const char *marker) {
        QElapsedTimer timer;
        timer.start();
        while (timer.elapsed() < 6000 && !contains(marker)) {
            view->poll_and_update();
            QCoreApplication::processEvents();
            QThread::msleep(10);
        }
        return contains(marker);
    };
    const bool ready = wait_for(">"); // screen_text trims a prompt's trailing space.
    QInputMethodEvent input;
    input.setCommitString(QStringLiteral("Write-Output ('QT_' + 'PWSH_' + $PSVersionTable.PSVersion.Major)"));
    QApplication::sendEvent(view, &input);
    QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
    QApplication::sendEvent(view, &enter);
    const bool echoed = wait_for("QT_PWSH_7") && view->painted() > 0;
    const auto status = term_status(tab);
    std::printf("[cpp] C4 saved pwsh -> Qt input/renderer/PTY: focused=%d ready=%d echoed=%d status=%.*s\n",
        focused, ready, echoed, int(status.size()), status.data());
    if (!echoed) {
        const auto screen = term_screen_text(tab);
        std::printf("[cpp] C4 pwsh fixture reading=%d painted=%u screen=[%.*s]\n",
            term_reading(tab), view->painted(), int(screen.size()), screen.data());
    }
    window->close_tab(page);
    return sess_delete(id) && focused && ready && echoed;
}
#endif

static bool check_local_options(QWidget *parent) {
    for (const char *shell : {"pwsh", "git-bash"}) {
        SessCfg draft = sess_form_get("", 0);
        draft.local_shell = "auto";
        draft.local_path = "C:/tools with spaces/shell.exe";
        draft.local_args = "-NoProfile -Command \"Write-Output 'form proof'\"";
        draft.local_cwd = "C:/working directory";
        SessionForm form(parent, draft);
        QComboBox *picker = nullptr;
        for (auto *combo : form.findChildren<QComboBox *>())
            if (combo->findText(QStringLiteral("pwsh")) >= 0
                && combo->findText(QStringLiteral("git-bash")) >= 0) picker = combo;
        if (!picker) return false;
        picker->setCurrentText(QString::fromLatin1(shell));
        SessCfg collected;
        form.collect(&collected);
        const auto id = sess_save(collected);
        if (id.empty()) return false;
        SessionForm edit(parent, sess_form_get(id, 0));
        SessCfg restored;
        edit.collect(&restored);
        const bool ok = restored.kind == 0 && restored.local_shell == shell
            && restored.local_path == draft.local_path && restored.local_args == draft.local_args
            && restored.local_cwd == draft.local_cwd;
        const bool removed = sess_delete(id);
        if (!ok || !removed) return false;
    }
#ifdef Q_OS_WIN
    return check_pwsh_terminal(parent);
#else
    return true;
#endif
}

bool check_session_options(QWidget *parent) {
    if (!check_local_options(parent)) return false;
    for (bool binary : {false, true}) {
        auto tcp = sess_form_get("", 4);
        tcp.tcp_host = "tcp.example";
        tcp.tcp_binary = binary;
        SessionForm tcp_form(parent, tcp);
        SessCfg restored;
        tcp_form.collect(&restored);
        if (restored.kind != 4 || restored.tcp_binary != binary) {
            std::printf("[session] TCP mode roundtrip failed: expected=%d actual=%d\n",
                        int(binary), int(restored.tcp_binary));
            return false;
        }
    }
    SessCfg draft = sess_form_get("", 1);
    draft.ssh_host = "10.0.0.9";
    draft.ssh_user = "admin";
    draft.ssh_password = "must-not-be-default";
    SessionForm form(parent, draft);
    auto *sftp = form.findChild<QCheckBox *>(QStringLiteral("sessionSftp"));
    auto *sysmon = form.findChild<QCheckBox *>(QStringLiteral("sessionSysmon"));
    if (!sftp || !sysmon) return false;
    auto *host_field = form.findChild<QLineEdit *>(QStringLiteral("sessionHost"));
    auto *port_field = form.findChild<QSpinBox *>(QStringLiteral("sessionPort"));
    auto *tabs = form.findChild<QTabBar *>(QStringLiteral("sessionProtocols"));
    if (!host_field || !port_field || !tabs) return false;
    host_field->setText(QStringLiteral("ssh.example")); port_field->setValue(2222);
    tabs->setCurrentIndex(2);
    host_field->setText(QStringLiteral("telnet.example")); port_field->setValue(2323);
    SessCfg telnet;
    form.collect(&telnet);
    if (telnet.kind != 2 || telnet.telnet_host != "telnet.example" || telnet.telnet_port != 2323) return false;
    tabs->setCurrentIndex(1);
    if (host_field->text() != QStringLiteral("ssh.example") || port_field->value() != 2222) return false;
    tabs->setCurrentIndex(2);
    if (host_field->text() != QStringLiteral("telnet.example") || port_field->value() != 2323) return false;
    tabs->setCurrentIndex(1);
    sftp->setChecked(false);
    sysmon->setChecked(true);
    QComboBox *charset = nullptr;
    for (auto *combo : form.findChildren<QComboBox *>())
        if (combo->findText(QStringLiteral("GBK")) >= 0) charset = combo;
    auto *path_words = form.findChild<QCheckBox *>(QStringLiteral("sessionOption_path_word_selection"));
    auto *mark_save = form.findChild<QCheckBox *>(QStringLiteral("sessionOption_mark_save_on_close"));
    if (!path_words || !mark_save) return false;
    path_words->setChecked(true); mark_save->setChecked(true);
    auto *delete_key = form.findChild<QComboBox *>(QStringLiteral("sessionOption_delete_mode"));
    auto *backspace = form.findChild<QComboBox *>(QStringLiteral("sessionOption_backspace_del"));
    auto *enter = form.findChild<QComboBox *>(QStringLiteral("sessionOption_enter_mode"));
    auto *proxy = form.findChild<QComboBox *>(QStringLiteral("sessionOption_proxy_kind"));
    auto *host = form.findChild<QLineEdit *>(QStringLiteral("sessionOption_proxy_host"));
    auto *password = form.findChild<QLineEdit *>(QStringLiteral("sessionOption_proxy_password"));
    auto *permissions = form.findChild<QSpinBox *>(QStringLiteral("sessionOption_sftp_file_mode"));
    auto *agent_forward = form.findChild<QCheckBox *>(QStringLiteral("sessionOption_agent_forwarding"));
    auto *x11 = form.findChild<QCheckBox *>(QStringLiteral("sessionOption_x11_enabled"));
    auto *display = form.findChild<QLineEdit *>(QStringLiteral("sessionOption_x11_display"));
    auto *authority = form.findChild<QLineEdit *>(QStringLiteral("sessionOption_x11_auth_file"));
    auto *no_auth = form.findChild<QCheckBox *>(QStringLiteral("sessionOption_x11_no_auth"));
    auto *modem_timeout = form.findChild<QSpinBox *>(QStringLiteral("sessionOption_modem_timeout_secs"));
    if (!delete_key || !backspace || !charset || !enter || !proxy || !host || !password || !permissions || !agent_forward || !x11 || !display || !authority || !no_auth || !modem_timeout) return false;
    agent_forward->setChecked(true); x11->setChecked(true);
    display->setText(QStringLiteral("127.0.0.1:0.1")); modem_timeout->setValue(25);
    authority->setText(QStringLiteral("fixture.Xauthority")); no_auth->setChecked(true);
    charset->setCurrentText(QStringLiteral("GBK"));
    delete_key->setCurrentIndex(2); backspace->setCurrentIndex(1);
    enter->setCurrentIndex(2); proxy->setCurrentIndex(1);
    host->setText(QStringLiteral("127.0.0.1")); password->setText(QStringLiteral("ephemeral-proxy-token"));
    permissions->setValue(0600);
    form.collect(&draft);
    if (!sess_validate(draft).empty()) return false;
    const auto id = sess_save(draft);
    if (id.empty()) return false;
    const auto saved = sess_form_get(id, 1);
    if (!saved.options.mark_save_on_close || !saved.options.path_word_selection || saved.sftp_enabled || !saved.sysmon_enabled || saved.charset != "GBK"
        || saved.options.enter_mode != 2 || saved.options.delete_mode != 2 || !saved.options.backspace_del || saved.options.proxy_kind != 1
        || saved.options.proxy_host != "127.0.0.1" || !saved.options.proxy_password.empty()
        || saved.options.sftp_file_mode != 0600 || !saved.options.agent_forwarding || !saved.options.x11_enabled
        || saved.options.x11_display != "127.0.0.1:0.1" || saved.options.modem_timeout_secs != 25
        || saved.options.x11_auth_file != "fixture.Xauthority" || !saved.options.x11_no_auth) return false;
    SessionForm edit(parent, saved);
    SessCfg edited;
    edit.collect(&edited);
    if (!edited.options.mark_save_on_close || !edited.options.path_word_selection || edited.sftp_enabled || !edited.sysmon_enabled || edited.options.enter_mode != 2
        || edited.options.delete_mode != 2 || !edited.options.backspace_del
        || edited.options.sftp_file_mode != 0600 || !edited.options.proxy_password.empty()
        || !edited.options.agent_forwarding || !edited.options.x11_enabled || edited.options.modem_timeout_secs != 25
        || edited.options.x11_display != "127.0.0.1:0.1" || edited.options.x11_auth_file != "fixture.Xauthority" || !edited.options.x11_no_auth) return false;
    if (!sess_defaults_save(draft).empty()) return false;
    const auto future = sess_form_get("", 1);
    SessionForm new_local(parent, sess_form_get("", 0));
    auto *protocols = new_local.findChild<QTabBar *>(QStringLiteral("sessionProtocols"));
    if (!protocols) return false;
    protocols->setCurrentIndex(1);
    SessCfg switched;
    new_local.collect(&switched);
    const bool switch_ok = !switched.sftp_enabled && switched.sysmon_enabled && switched.kind == 1
        && switched.charset == "GBK" && switched.options.enter_mode == 2 && switched.options.sftp_file_mode == 0600;
    protocols->setCurrentIndex(0);
    protocols->setCurrentIndex(1);
    new_local.collect(&switched);
    const bool ok = future.options.mark_save_on_close && future.options.path_word_selection && !future.sftp_enabled && future.sysmon_enabled && future.id.empty()
        && future.ssh_host.empty() && future.ssh_user.empty() && future.ssh_password.empty()
        && !sess_form_get("", 0).sysmon_enabled
        && future.options.agent_forwarding && future.options.x11_enabled
        && future.options.x11_display == "127.0.0.1:0.1" && future.options.modem_timeout_secs == 25
        && future.options.x11_auth_file == "fixture.Xauthority" && future.options.x11_no_auth;
    // Restore the defaults so later C4 cases keep their independent baseline.
    SessCfg clean = future;
    clean.sftp_enabled = true;
    clean.sysmon_enabled = false;
    clean.options.mark_save_on_close = false;
    clean.charset = "UTF-8";
    clean.options.enter_mode = 0; clean.options.proxy_kind = 0;
    clean.options.proxy_host = ""; clean.options.sftp_file_mode = 0644;
    clean.options.agent_forwarding = false; clean.options.x11_enabled = false;
    clean.options.x11_display = ""; clean.options.modem_timeout_secs = 10;
    clean.options.x11_auth_file = ""; clean.options.x11_no_auth = false;
    const bool restored = sess_defaults_save(clean).empty();
    return sess_delete(id) && restored && ok && switch_ok && !switched.sftp_enabled && switched.sysmon_enabled;
}
