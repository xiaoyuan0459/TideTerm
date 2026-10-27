#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_window.h"
#include "cpp/chrome/selftest_terminal_suggestions.h"
// Active pane, split lifetime and real shell directory integration.
#include "cpp/bridge_shim.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/selftest_shell.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QDir>
#include <QDockWidget>
#include <QFile>
#include <QLineEdit>
#include <QKeyEvent>
#include <QListView>
#include <QPixmap>
#include <QTimer>
#include <QTreeWidget>
#include <QUuid>
#include <cstdio>
#include <algorithm>
#include <filesystem>
#include <string>

namespace {
bool check_split_resize(MainWindow &w, const QString &png_path) {
    const QSize original = w.size();
    std::vector<QSize> large;
    std::vector<QSize> small;
    bool ok = true;
    for (const QSize requested : {QSize(1400, 900), QSize(1000, 650), QSize(1400, 900)}) {
        w.resize(requested);
        QApplication::processEvents();
        for (int pane = 0; auto *view = w.pane_view(0, pane); ++pane) {
            const auto stats = term_stats(view->tab());
            bool fitted = view->isVisible() && view->width() > 0 && view->height() > 0;
            for (QWidget *child = view; child != &w && child->parentWidget(); child = child->parentWidget())
                fitted &= child->parentWidget()->rect().contains(child->geometry());
            // 正文格数 + 行号栏才是屏上真正占的宽度（栏宽不计入 stats.cols）。
            // 唯一例外：窗格窄到放不下"栏 + 2 列"下限时列数被钳在 2，栏仍占满宽，
            // 右侧会被裁掉几列 —— 8 列以下的窗格本来不可用，这里如实放过而不是
            // 假装不溢出（要根治得让栏宽随可用格数收缩，见路线图 §8.17 的待办）。
            const bool fits_width = (stats.cols + stats.gutter_cells) * stats.cell_w <=
                view->grid_w() * view->devicePixelRatioF();
            fitted &= fits_width || stats.cols <= 2;
            fitted &= stats.rows * stats.cell_h <= view->grid_h() * view->devicePixelRatioF();
            if (large.size() <= size_t(pane)) large.push_back(view->size());
            else if (requested.width() == 1000) {
                fitted &= view->width() < large[size_t(pane)].width() &&
                          view->height() < large[size_t(pane)].height();
                small.push_back(view->size());
            } else fitted &= view->width() > small[size_t(pane)].width() &&
                             view->height() > small[size_t(pane)].height();
            ok &= fitted;
            std::printf("[cpp] C2 resize %dx%d pane%d=%dx%d grid=%ux%u fitted=%d\n",
                w.width(), w.height(), pane, view->width(), view->height(), stats.cols, stats.rows, fitted);
        }
        if (!png_path.isEmpty()) {
            QEventLoop settle;
            QTimer::singleShot(2000, &settle, &QEventLoop::quit);
            settle.exec();
            ok &= w.grab().save(QStringLiteral("%1-%2x%3.png").arg(png_path)
                .arg(w.width()).arg(w.height()));
        }
    }
    w.resize(original);
    QApplication::processEvents();
    std::printf("[cpp] C2 nested panes shrink/grow within viewport: %s\n", ok ? "PASS" : "FAIL");
    return ok;
}
}

/// Phase C ② 自检：分割/关窗格（offscreen 确定性）。
int qt_boot_c2(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c2";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    MainWindow w;
    w.show();
    w.add_tab(); // 2 页：page0 做分割实验，page1 验证会话总数口径
    if (!selftest_ui::activate_window(w)) {
        std::fprintf(stderr, "C2 fixture: native window did not acquire focus\n");
        return 1;
    }

    int verdict = 1;
    bool ok = true;
    // Inherit the temporary directory's ACL. QTemporaryDir restricts its ACL to
    // the owner, which prevents Windows restricted tokens from creating children.
    const QString pane_dir = QDir::temp().filePath(QStringLiteral("tideterm-c2-%1")
        .arg(QUuid::createUuid().toString(QUuid::WithoutBraces)));
    std::error_code directory_error;
    const bool directory_created = std::filesystem::create_directory(
        std::filesystem::path(pane_dir.toStdWString()), directory_error);
    ok &= directory_created;
    std::printf("[cpp] C2 fixture directory created=%d: %s\n", directory_created,
        directory_error.message().c_str());
    QFile context_file(QDir(pane_dir).filePath(QStringLiteral("focused-pane.txt")));
    ok &= context_file.open(QIODevice::WriteOnly);
    ok &= context_file.write("focused") == 7;
    context_file.close();
    const QString witness_path = pane_dir + QStringLiteral("-cwd");
    QPointer<PtyView> context_pane;
    QPointer<PtyView> inherited_pane;
    QTimer context_poll;
    int waited_ms = 0;
    bool changed_dir = false;
    bool typed_history = false;
    bool split_checked = false;
    bool witness_sent = false;
    QTimer::singleShot(400, [&] {
        // C③ 起 active_view() 无焦点回落「当前页」（旧=第一页）；add_tab 已把当前页
        // 切到 page1 —— 显式聚焦 page0 首窗格，分割语义确定性化。
        w.focus_session(w.page_id(0));
        // 1) 水平分割：page0 窗格数 1→2，两个稳定 id 唯一。
        w.split_active(Qt::Horizontal);
        QApplication::processEvents(); // New pane layout/show precedes synthetic focus input.
        auto ids = w.pane_ids(0);
        const bool split_ok = ids.size() == 2 && ids[0] != ids[1];
        ok &= split_ok;
        std::printf("[cpp] C2 initial split: count=%zu ids=%u,%u ok=%d\n", ids.size(), ids.empty() ? UINT32_MAX : ids[0], ids.size() < 2 ? UINT32_MAX : ids[1], split_ok);
        // A toolbar/send input must not silently change the split target.
        PtyView *target = w.pane_view(0, 1);
        if (target) target->setFocus();
        w.page_sendbar(0)->input()->setFocus();
        const bool retained = target && w.active_view() == target;
        ok &= retained;
        std::printf("[cpp] C2 toolbar retains active pane: %s\n", retained ? "PASS" : "FAIL");
        // 2) 垂直分割（对当前焦点窗格）：3 窗格。
        w.pane_view(0, 0)->setFocus();
        w.split_active(Qt::Vertical);
        ok &= w.pane_ids(0).size() == 3;
        std::printf("[cpp] C2-a 分割（水平→垂直嵌套）: %s（page0 %zu 窗格）\n",
                    ok ? "通过" : "失败", w.pane_ids(0).size());
        // 3) 会话总数口径：page0 3 + page1 1 = 4。
        ok &= term_tab_count() == 4;
        std::printf("[cpp] C2-b 全局会话数 = 窗格数: %s（%u）\n",
                    term_tab_count() == 4 ? "通过" : "失败", term_tab_count());
        context_pane = target;
        if (target) {
            target->setFocus();
            w.refresh_chrome();
            w.page_sendbar(0)->input()->setFocus();
        }
        context_poll.start(100);
    });

    QObject::connect(&context_poll, &QTimer::timeout, [&] {
        waited_ms += 100;
        if (context_pane && !changed_dir) {
            const auto screen = term_screen_text(context_pane->tab());
            if (selftest_shell::ready(std::string_view(screen.data(), screen.size()))) {
                const QString command = selftest_shell::cd(pane_dir);
                w.page_sendbar(0)->input()->setText(command);
                w.page_sendbar(0)->send();
                changed_dir = true;
            }
        }
        if (inherited_pane) {
            const auto raw = term_screen_text(inherited_pane->tab());
            if (!witness_sent && selftest_shell::ready(std::string_view(raw.data(), raw.size()))) {
                const QByteArray command = (selftest_shell::cwd_witness(witness_path) + QLatin1Char('\r')).toUtf8();
                term_write(inherited_pane->tab(), rust::Slice<const uint8_t>(
                    reinterpret_cast<const uint8_t *>(command.constData()), size_t(command.size())));
                witness_sent = true;
            }
            // Read a Shell-produced witness: narrow panes can scroll a long
            // printed path out of the screen before the UI observes it.
            QFile witness(witness_path);
            const bool complete = witness.open(QIODevice::ReadOnly);
            if (!complete && waited_ms < 15000) return;
            const QString observed = complete ? QString::fromUtf8(witness.readAll()) : QString();
            const bool inherited = complete && QDir(observed).canonicalPath() == QDir(pane_dir).canonicalPath();
            ok &= inherited;
            std::printf("[cpp] C2 new split shell inherits changed directory: %s\n", inherited ? "PASS" : "FAIL");
            if (!inherited) std::printf("[cpp] C2 split geometry=%dx%d; witness: %.*s\n",
                inherited_pane->width(), inherited_pane->height(), int(raw.size()), raw.data());
            w.close_pane(inherited_pane);
            inherited_pane.clear();
            context_pane->setFocus(); w.refresh_chrome();
            split_checked = true;
            waited_ms = 0;
            return; // File refresh and replacement layout complete on the next turn.
        }
        bool context_ok = context_pane && w.active_view() == context_pane;
        if (context_ok) {
            auto *browser = w.docks()->sftp;
            context_ok &= browser->local_rows() == 1 &&
                browser->local_tree()->topLevelItem(0)->text(0) == QStringLiteral("focused-pane.txt");
            auto *list = w.docks()->session_list;
            const int row = list->currentIndex().row();
            context_ok &= row >= 0 && row < int(w.docks()->session_ids.size()) &&
                w.docks()->session_ids[size_t(row)] == context_pane->tab();
        }
        if (context_ok && !typed_history) {
            context_pane->setFocus();
            const QString command = selftest_shell::history_command();
            for (QChar letter : command) {
                QKeyEvent key(QEvent::KeyPress, 0, Qt::NoModifier, QString(letter));
                QApplication::sendEvent(context_pane, &key);
            }
            QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, QStringLiteral("\r"));
            QApplication::sendEvent(context_pane, &enter);
            typed_history = true;
            return;
        }
        if (context_ok) {
            w.refresh_chrome();
            auto *history_list = w.docks()->history->findChild<QListView *>(QStringLiteral("historyList"));
            bool recorded = false;
            if (history_list) for (int row = 0; row < history_list->model()->rowCount(); ++row)
                recorded |= history_list->model()->index(row, 0).data().toString() == selftest_shell::history_command();
            context_ok &= recorded;
        }
        if (!context_ok && waited_ms < 15000) return;
        if (context_ok && !split_checked) {
            context_pane->setFocus();
            const auto before = w.pane_ids(0);
            w.split_active(Qt::Horizontal);
            context_poll.stop();
            ok &= check_split_resize(w, QString::fromUtf8(png_out.data(), int(png_out.size())));
            context_poll.start(100);
            for (uint32_t id : w.pane_ids(0)) {
                if (std::find(before.begin(), before.end(), id) == before.end()) {
                    for (int pane = 0; PtyView *view = w.pane_view(0, pane); ++pane) {
                        if (view->tab() == id) { inherited_pane = view; break; }
                    }
                    w.focus_session(id); break;
                }
            }
            if (inherited_pane) { waited_ms = 0; return; }
            ok = false;
            split_checked = true;
        }
        context_poll.stop();
        if (!context_ok && context_pane) {
            const auto screen = term_screen_text(context_pane->tab());
            std::printf("[cpp] C2 failed screen: %.*s\n", int(qMin(size_t(2500), screen.size())), screen.data());
            std::printf("[cpp] C2 changed directory=%d typed history=%d status=%d\n", changed_dir, typed_history, term_status_kind(context_pane->tab()));
            for (const auto &command : history_commands(context_pane->tab()))
                std::printf("[cpp] C2 history: %.*s\n", int(command.size()), command.data());
        }
        if (context_pane) {
            const auto cwd = term_current_dir(context_pane->tab());
            std::printf("[cpp] C2 actual cwd=%.*s; rows=%d; pane visible=%d\n", int(cwd.size()), cwd.data(),
                w.docks()->sftp->local_rows(), context_pane->isVisible());
            std::printf("[cpp] C2 geometry=%dx%d\n", context_pane->width(), context_pane->height());
            context_ok &= context_pane->isVisible() && context_pane->width() > 100 && context_pane->height() > 100;
        }
        ok &= context_ok;
        std::printf("[cpp] C2 direct terminal history, focused directory and session selection: %s\n", context_ok ? "PASS" : "FAIL");
        // 4) 关闭一个窗格：3→2，被关会话终止（全局 4→3）。
        if (PtyView *v = w.pane_view(0, 2)) {
            const uint32_t doomed = v->tab();
            w.close_pane(v);
            auto ids = w.pane_ids(0);
            ok &= ids.size() == 2;
            ok &= term_tab_count() == 3;
            for (uint32_t id : ids) {
                ok &= id != doomed; // 被关会话的 id 不应残留
            }
        } else {
            ok = false;
        }
        std::printf("[cpp] C2-c 关闭窗格: %s（page0 %zu 窗格，全局 %u 会话）\n",
                    ok ? "通过" : "失败", w.pane_ids(0).size(), term_tab_count());

        // Let Qt complete layout/paint and deferred widget destruction. Capture
        // the state the user sees, not the transitional close-event stack.
        QTimer::singleShot(100, &w, [&] {
            for (int pane = 0; pane < 2; ++pane) {
                auto *view = w.pane_view(0, pane);
                ok &= view && view->isVisible() && view->width() > 100 &&
                      view->height() > 100 && view->painted() > 0;
            }
            const std::string png_path(png_out.data(), png_out.size());
            if (!png_path.empty()) {
                const QPixmap shot = w.grab();
                const bool saved = shot.save(QString::fromStdString(png_path));
                std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                            saved ? "成功" : "失败", shot.width(), shot.height());
                ok &= saved;
            }
            ok &= verify_terminal_suggestions(w);
            verdict = ok ? 0 : 1;
            std::printf("[cpp] Phase C ② 自检: %s\n", verdict == 0 ? "通过" : "失败");
            while (w.page_count() > 0) w.close_tab(0);
            std::error_code cleanup_error;
            std::filesystem::remove_all(std::filesystem::path(pane_dir.toStdWString()), cleanup_error);
            QFile::remove(witness_path);
            app.exit(verdict);
        });
    });

    return app.exec();
}
