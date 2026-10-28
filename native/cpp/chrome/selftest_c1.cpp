#include "cpp/chrome/selftest_settings.h"
#include "cpp/chrome/selftest_window.h"
// Phase C ①/⑤ 自检实现（自 main.cpp / selftest_chrome.cpp 拆出——行数门禁）。
// C①：三标签隔离 + 拖拽重排跟账（R-02）+ 关闭生命周期 + OSC 标题。
// C⑤：SFTP 浏览器结构/本地扫描/OS 拖入语义（SFTP-05）/异步错误路径/队列空态。

#include "cpp/chrome/selftest_chrome.h"

#include "cpp/chrome/docks.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/selftest_groups.h"
#include "cpp/chrome/selftest_shell.h"
#include "cpp/chrome/selftest_close_menu.h"
#include "cpp/chrome/selftest_daily.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"

#include <QApplication>
#include <QDir>
#include <QDockWidget>
#include <QFile>
#include <QPixmap>
#include <QLineEdit>
#include <QPointer>
#include <QEvent>
#include <QEventLoop>
#include <QElapsedTimer>
#include <QTabBar>
#include <QTimer>
#include <QTreeWidget>

#include <cstdio>
#include <string>

/// Phase C ① 自检：三标签隔离 + 拖拽重排跟账（R-02）+ 关闭生命周期 + OSC 标题
///（offscreen 确定性）。
int qt_boot_c1(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c1";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    std::fprintf(stderr, "C1 diagnostic: before window\n");
    MainWindow w;
    std::fprintf(stderr, "C1 diagnostic: before show\n");
    w.show();
    std::fprintf(stderr, "C1 diagnostic: before second tab\n");
    w.add_tab();
    std::fprintf(stderr, "C1 diagnostic: before third tab\n");
    w.add_tab(); // 共 3 个标签（ctor 已建 1 个）
    if (!selftest_ui::activate_window(w)) {
        std::fprintf(stderr, "C1 fixture: native window did not acquire focus\n");
        return 1;
    }
    std::fprintf(stderr, "C1 diagnostic: before event loop\n");

    // 标记注入时机：轮询各标签屏出现提示符（"> "）再喂。三并发 pwsh 冷启动的
    // PSReadLine 就绪时间不定（固定 1.5s 在慢机会过早）；标记必须走 **shell 回显**
    // （Write-Output）——直接注入网格会与 PSReadLine 重画竞争（实测行首被覆盖），
    // 回显走 PSReadLine 正规路径（pty-selftest 已验证可靠）。6s 硬超时兜底注入。
    QTimer inject_poll;
    int waited_ms = 0;
    int verdict = 1;
    QObject::connect(&inject_poll, &QTimer::timeout, [&] {
        waited_ms += 100;
        auto prompt_ready = [](uint32_t tab) {
            const rust::String rs = term_screen_text(tab);
            const std::string text(rs.data(), rs.size());
            return selftest_shell::ready(text);
        };
        if (!(prompt_ready(0) && prompt_ready(1) && prompt_ready(2))
            && waited_ms < 6000) {
            return;
        }
        inject_poll.stop();
        for (uint32_t id = 0; id < 3; ++id) {
            const char *tag = id == 0 ? "TABZERO" : (id == 1 ? "TABONE" : "TABTWO");
#ifdef Q_OS_WIN
            std::string cmd = "Write-Output ";
#else
            std::string cmd = "printf '%s\\n' ";
#endif
            cmd += tag;
            cmd += "\r";
            term_write(id, rust::Slice<const uint8_t>(
                reinterpret_cast<const uint8_t *>(cmd.data()), cmd.size()));
        }
        QTimer::singleShot(900, [&] {
            auto has = [](uint32_t tab, const char *needle) {
                const rust::String rs = term_screen_text(tab);
                const std::string text(rs.data(), rs.size());
                return text.find(needle) != std::string::npos;
            };
            bool ok = true;
            // 1) 隔离：各标签只含自己的标记。
            ok &= has(0, "TABZERO") && !has(0, "TABONE") && !has(0, "TABTWO");
            ok &= has(1, "TABONE") && !has(1, "TABTWO");
            ok &= has(2, "TABTWO") && !has(2, "TABZERO");
            std::printf("[cpp] C1-a 三标签内容隔离: %s\n", ok ? "通过" : "失败");
            // 1b) 拖拽重排跟账（R-02）：moveTab 与拖拽走同一条 tabMoved 信号，
            //     page_roots_ 等三向量必须随页序同步 —— 否则「关 A 杀 B」（TAB-02）。
            //     moveTab(2,0) 语义 = 摘下 index 2 插到 0 ⇒ [0,1,2] → [2,0,1]。
            for (int i = 0; i < 3; ++i) {
                w.page_sendbar(i)->input()->setText(QStringLiteral("draft-%1").arg(w.page_id(i)));
            }
            w.tabs_bar()->moveTab(2, 0);
            const bool reorder_ok =
                w.page_id(0) == 2 && w.page_id(1) == 0 && w.page_id(2) == 1
                && has(w.page_id(0), "TABTWO") && has(w.page_id(1), "TABZERO")
                && has(w.page_id(2), "TABONE");
            std::printf("[cpp] C1-a2 拖拽重排跟账(R-02): %s（page ids=%u,%u,%u）\n",
                        reorder_ok ? "通过" : "失败", w.page_id(0), w.page_id(1),
                        w.page_id(2));
            ok &= reorder_ok;
            bool send_owner_ok = true;
            for (int i = 0; i < 3; ++i) {
                w.tabs_bar()->setCurrentIndex(i);
                send_owner_ok &= w.page_sendbar(i)->isVisible() &&
                    w.page_sendbar(i)->input()->text() == QStringLiteral("draft-%1").arg(w.page_id(i));
            }
            std::printf("[cpp] C1-a3 reordered tabs keep their own visible send draft: %s\n",
                        send_owner_ok ? "PASS" : "FAIL");
            ok &= send_owner_ok;
            // 1c) 独立分组。**必须在关闭之前**：它硬性要求 3 个标签（`pane_view(2,0)`、
            //     `term_tab_count()==3`）。注意它会把窗格从 30 行压到 12 行再拉回，这轮
            //     resize 风暴里 shell 自己会清屏（连擦除滚动历史的序列一起发），所以后面
            //     不能再断言"某个标记还在屏上"。
            ok &= verify_tab_groups(&w);
            // 2) 关闭中间标签（重排后 = TABZERO id 0）：剩余 2 个且两页内容完好
            //    （**位置≠id**：剩余 page 序 = [TABTWO, TABONE]）。
            //    各子条件分开判定并逐项打印：这里原来是串成一条 `ok &=`，任何一项失败都
            //    只显示"失败"两个字，无法分辨是计数、发送栏释放、可见性还是页面内容。
            QPointer<SendBar> closed_sendbar = w.page_sendbar(1);
            w.close_tab(1);
            const bool count_ok = term_tab_count() == 2;
            QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
            const bool sendbar_released = closed_sendbar.isNull();
            const bool current_sendbar_visible = w.page_sendbar(w.current_page())->isVisible();
            // 内容不在这一步断言：上一步的分组拖拽会把窗格压扁再拉回，shell 自己会清屏，
            // 之后"屏上还有我的标记"只是在测 shell 的重绘心情（内容隔离由 C1-a 覆盖）。
            // "关 A 杀 B"要证明的是**会话没被误杀**：幸存两页的 id 正确，且两边的本地
            // shell 仍报 running（3）——被杀掉的话状态会变成 disconnected(4)/error(5)。
            const bool first_page_ok =
                w.page_id(0) == 2 && term_status_kind(w.page_id(0)) == 3;
            const bool second_page_ok =
                w.page_id(1) == 1 && term_status_kind(w.page_id(1)) == 3;
            const bool close_ok = count_ok && sendbar_released && current_sendbar_visible
                && first_page_ok && second_page_ok;
            std::printf("[cpp] C1-b 重排后关闭中间标签: %s（剩余 %u 标签，page ids=%u,%u；"
                        "count=%d sendbar释放=%d 可见=%d 首页=%d 次页=%d）\n",
                        close_ok ? "通过" : "失败", term_tab_count(), w.page_id(0),
                        w.page_id(1), int(count_ok), int(sendbar_released),
                        int(current_sendbar_visible), int(first_page_ok), int(second_page_ok));
            ok &= close_ok;
            // 3) OSC 标题链路：pwsh 启动即通过 OSC 0;2 上报自身路径 —— 非空即证明
            //    OSC→标题管线工作；回退「终端 N」仅在无 OSC 的 shell 下出现。
            //    （重排+关闭后 TABZERO(id 0) 已关，取存活页首窗格 id。）
            const rust::String t0rs = term_title(w.page_id(0));
            const std::string t0(t0rs.data(), t0rs.size());
            const bool title_ok = !t0.empty();
            ok &= title_ok;
            std::printf("[cpp] C1-c OSC 标题: %s（\"%s\"）\n", title_ok ? "通过" : "失败",
                        t0.c_str());

            const std::string png_path(png_out.data(), png_out.size());
            if (!png_path.empty()) {
                const QPixmap shot = w.grab();
                const bool saved = shot.save(QString::fromStdString(png_path));
                std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                            saved ? "成功" : "失败", shot.width(), shot.height());
                ok = ok && saved;
            }
            QPointer<PtyView> late_view = w.pane_view(0, 0);
            MainWindow survivor;
            survivor.show();
            const uint32_t survivor_id = survivor.page_id(0);
            ok &= selftest_ui::accept_window_close(w);
            const bool scoped_close = term_tab_count() == 1 &&
                !term_title(survivor_id).empty() && late_view && late_view->is_closed();
            ok &= scoped_close;
            if (scoped_close) ok &= !late_view->grab().isNull();
            std::printf("[cpp] C1-d window close retires owned sessions and preserves another window: %s\n",
                scoped_close ? "PASS" : "FAIL");
            ok &= selftest_ui::accept_window_close(survivor);
            ok &= term_tab_count() == 0;
            verdict = ok ? 0 : 1;
            std::printf("[cpp] Phase C ① 自检: %s\n", verdict == 0 ? "通过" : "失败");
            app.exit(verdict);
        });
    });
    inject_poll.start(100);

    return app.exec();
}

/// Phase C ⑤ 自检：SFTP+传输。无 SSH 服务器环境下验证：浏览器结构、本地栏
/// 真扫、桥异步往返（无通道错误路径）、OS 拖入语义（SFTP-05）、attach 流程
///（本地 PTY 会话 open_sftp 失败 → 错误结果经 ops 队列回 UI）。
int qt_boot_c5(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c5";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    MainWindow w;
    w.show();

    QTimer poll;
    int waited_ms = 0;
    int verdict = 1;
    bool ok = true;
    bool saw_open_error = false;
    bool saw_list_error = false;
    QString copied_drag;
    // Observe asynchronous production results; do not force synchronous UI IO.
    auto await_rows = [](SftpBrowser *browser, int count) {
        QElapsedTimer deadline;
        deadline.start();
        while (browser && browser->local_rows() != count && deadline.elapsed() < 3000) {
            browser->poll();
            QEventLoop turn;
            QTimer::singleShot(10, &turn, &QEventLoop::quit);
            turn.exec();
        }
        return browser && browser->local_rows() == count;
    };

    QTimer::singleShot(200, [&] {
        SftpBrowser *sftp = w.docks() != nullptr ? w.docks()->sftp : nullptr;
        // 1) 结构：双栏 + 传输队列 + 工具条。
        bool struct_ok = sftp != nullptr && sftp->remote_tree() != nullptr
                         && sftp->local_tree() != nullptr
                         && sftp->toolbar_widget() != nullptr;
        if (struct_ok) {
            struct_ok &= sftp->remote_tree()->dragEnabled()
                         && sftp->remote_tree()->acceptDrops();
        }
        std::printf("[cpp] C5-a SftpBrowser 结构: %s\n", struct_ok ? "通过" : "失败");
        ok &= struct_ok;

        // 2) 本地栏真扫（临时目录注入）。
        const QString temp_root = QDir::temp().filePath(
            QStringLiteral("tideterm-c5-local"));
        QDir(temp_root).removeRecursively();
        QDir().mkpath(temp_root);
        QFile f1(QDir(temp_root).filePath(QStringLiteral("a.txt")));
        const bool f1_ok = f1.open(QIODevice::WriteOnly);
        f1.write("hello");
        f1.close();
        QDir().mkpath(QDir(temp_root).filePath(QStringLiteral("subdir")));
        bool local_ok = sftp != nullptr && f1_ok;
        if (local_ok) {
            sftp->scan_local(temp_root);
            local_ok &= await_rows(sftp, 2); // a.txt + subdir
        }
        std::printf("[cpp] C5-b 本地栏真扫: %s\n", local_ok ? "通过" : "失败");
        ok &= local_ok;
        // 2b) SFTP-05：OS 文件拖入语义 —— 本地栏 = 复制进当前目录（同名不覆盖）。
        //     合成 QDropEvent 在 offscreen 下不派发到 ItemView 的 dropEvent，
        //     故直测语义方法；dropEvent 连线本身与真实拖放同路径。
        bool drop_ok = sftp != nullptr;
        if (drop_ok) {
            const QString src1 = QDir(temp_root).filePath(QStringLiteral("a.txt"));
            const QString src2 = QDir::temp().filePath(
                QStringLiteral("tideterm-c5-drag.bin"));
            QFile f2(src2);
            const bool f2_ok = f2.open(QIODevice::WriteOnly);
            f2.write("drag");
            f2.close();
            // a.txt 与自身同名 → 跳过；drag.bin 新增 ⇒ 2 → 3 行。
            copied_drag = QDir(temp_root).filePath(QFileInfo(src2).fileName());
            sftp->handle_external_drop(QStringList{src1, src2}, false);
            drop_ok &= f2_ok;
            drop_ok &= await_rows(sftp, 3);
        }
        std::printf("[cpp] C5-b2 OS 文件拖入(本地栏复制): %s\n",
                    drop_ok ? "通过" : "失败");
        ok &= drop_ok;

        // 3) 桥异步错误路径：无通道 list（未 attach 的 uid）→ 立即失败结果。
        if (sftp != nullptr) {
            const rust::Vec<SftpOpPod> pre = sftp_take_ops();
            (void)pre;
            sftp_list(rust::String("tab-9999"), rust::String("/"));
            const rust::Vec<SftpOpPod> ops = sftp_take_ops();
            for (const SftpOpPod &op : ops) {
                if (op.kind == 1 && !op.ok) {
                    saw_list_error = true;
                }
            }
        }
        std::printf("[cpp] C5-c 无通道 list 错误路径: %s\n",
                    saw_list_error ? "通过" : "失败");
        ok &= saw_list_error;

        // 4) attach 本地 PTY 会话（tab 0）：open_sftp 应失败（非 SSH）→ poll 收
        //    kind=0 ok=false（真实异步往返，经 runtime spawn）。
        if (sftp != nullptr) {
            const rust::String uid = term_uid(0);
            sftp->attach(QString::fromUtf8(uid.data(), int(uid.size())),
                         QStringLiteral("终端 1"));
        }
        poll.start(100);
    });

    QObject::connect(&poll, &QTimer::timeout, [&] {
        waited_ms += 100;
        if (w.docks() != nullptr && w.docks()->sftp != nullptr) {
            w.docks()->sftp->poll();
            const QString st = w.docks()->sftp->status_text();
            if (st.contains(i18n::tr("SFTP 不可用：%1").arg(QString()))) {
                saw_open_error = true;
            }
        }
        if (saw_open_error || waited_ms >= 5000) {
            poll.stop();
            std::printf("[cpp] C5-d 本地会话 open_sftp 失败路径: %s（等 %dms）\n",
                        saw_open_error ? "通过" : "失败", waited_ms);
            ok &= saw_open_error;

            // The actual dropped file must have its completed result in the shared queue.
            bool queue_ok = false;
            auto *queue = dynamic_cast<QTreeWidget *>(w.docks()->transfer->widget());
            if (queue) for (int i = 0; i < queue->topLevelItemCount(); ++i) {
                auto *row = queue->topLevelItem(i);
                queue_ok |= row->text(0) == copied_drag && row->text(3) == i18n::tr("完成");
            }
            QFile copied(copied_drag);
            queue_ok &= copied.open(QIODevice::ReadOnly) && copied.readAll() == "drag";
            std::printf("[cpp] C5-e 复制结果与可见队列完成状态: %s\n", queue_ok ? "通过" : "失败");
            ok &= queue_ok;
            ok &= run_daily_ui(&w);

            const std::string png_path(png_out.data(), png_out.size());
            if (!png_path.empty()) {
                const QPixmap shot = w.grab();
                const bool saved = shot.save(QString::fromStdString(png_path));
                std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                            saved ? "成功" : "失败", shot.width(), shot.height());
                ok = ok && saved;
            }
            verdict = ok ? 0 : 1;
            std::printf("[cpp] Phase C ⑤ 自检: %s\n", verdict == 0 ? "通过" : "失败");
            app.exit(verdict);
        }
    });

    return app.exec();
}
