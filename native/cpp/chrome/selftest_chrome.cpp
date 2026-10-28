#include "cpp/chrome/selftest_settings.h"
// Phase C ③④ 自检实现（从 main.cpp 拆出——行数门禁；内容与拆分前一致）。

#include "cpp/i18n.h"
#include "cpp/chrome/selftest_chrome.h"
#include "cpp/chrome/selftest_groups.h"
#include "cpp/chrome/selftest_shell.h"
#include "cpp/chrome/selftest_close_menu.h"

#include "cpp/chrome/docks.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/session_form.h"
#include "cpp/chrome/selftest_session_options.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/main_window.h"

#include <QAction>
#include <QFile>
#include <QApplication>
#include <QDir>
#include <QDockWidget>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QPixmap>
#include <QTreeWidget>
#include <QTimer>
#include <QTabBar>

#include "cpp/chrome/session_banner.h"

#include <cstdio>
#include <functional>
#include <string>

/// Phase C ③ 自检：chrome 骨架（菜单/横幅+面包屑/发送栏/空态 dock/树模型）。
/// 结构同 c1：定时器/判定变量在**函数作用域**——lambda 栈帧内声明 QTimer 会随
/// 帧析构，其回调引用悬垂即挂死（首跑实测根因，勿再犯）。
int qt_boot_c3(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c3";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    MainWindow w;
    w.show();

    QTimer inject_poll;
    int waited_ms = 0;
    int verdict = 1;
    bool ok = true;

    QTimer::singleShot(100, [&] {
        ok &= selftest_ui::capture_main_menus(w, QString::fromUtf8(png_out.data(), int(png_out.size())));
        // 1) 菜单完整化：11 顶级菜单；递归收集全部叶子动作按 tid 断言。
        const auto top_menus = w.menuBar()->actions();
        ok &= top_menus.size() == 11;
        QVector<QAction *> leaves;
        std::function<void(QMenu *)> collect = [&](QMenu *m) {
            for (QAction *a : m->actions()) {
                if (a->menu()) {
                    collect(a->menu());
                } else if (!a->isSeparator()) {
                    leaves.push_back(a);
                }
            }
        };
        for (QAction *t : top_menus) {
            if (t->menu()) {
                collect(t->menu());
            }
        }
        auto find_action = [&leaves](const char *tid) -> QAction * {
            const QByteArray want(tid);
            for (QAction *a : leaves) {
                if (a->property("tid").toByteArray() == want) {
                    return a;
                }
            }
            return nullptr;
        };
        const int n_actions = int(leaves.size());
        QAction *a_new = find_action("session:new");
        QAction *a_hex = find_action("view:hex");
        QAction *a_mbar = find_action("view:toggle-menu-bar");
        ok &= n_actions >= 80;
        ok &= a_new != nullptr && a_new->shortcut().toString() == QStringLiteral("Alt+N");
        ok &= a_hex != nullptr && !a_hex->isEnabled();    // canonical 禁用项
        ok &= a_mbar != nullptr && a_mbar->isCheckable(); // canonical 可勾选项
        std::printf("[cpp] C3-a 菜单完整化: %s（顶级=%d 叶子=%d）\n",
                    top_menus.size() == 11 && n_actions >= 80 && a_new != nullptr
                            && a_hex != nullptr && a_mbar != nullptr
                        ? "通过"
                        : "失败",
                    int(top_menus.size()), n_actions);

        ok &= verify_panel_controls(&w);
        ok &= verify_send_pacing();
        // 2) PodTreeModel：与 Rust sess_tree_rows 快照一致（④ 起数据源 =
        // sessions.json；数据注入断言在 c4 的 hermetic store，此处验证模型↔桥一致）。
        bool tree_ok = w.docks() != nullptr && w.docks()->tree_model != nullptr;
        if (tree_ok) {
            PodTreeModel *model = w.docks()->tree_model;
            const rust::Vec<SessRow> rows = sess_explorer_rows();
            tree_ok &= model->total_nodes() == int(rows.size());
            std::printf("[cpp] C3-b 连接树: %s（模型节点=%d Rust 快照=%zu）\n",
                        tree_ok ? "通过" : "失败", model->total_nodes(), rows.size());
        } else {
            std::printf("[cpp] C3-b 连接树: 失败（模型缺失）\n");
        }
        ok &= tree_ok;

        // 3) Address bar reflects the focused local session.
        bool crumb_ok = w.page_banner(0) != nullptr;
        if (crumb_ok) {
            const QString crumb = w.page_banner(0)->breadcrumb();
            crumb_ok &= crumb.startsWith(QStringLiteral("shell://"));
            std::printf("[cpp] C3-c 面包屑: %s（\"%s\"）\n", crumb_ok ? "通过" : "失败",
                        crumb.toUtf8().constData());
        }
        ok &= crumb_ok;

        // 4) 文件管理器（⑤ 起 = SftpBrowser 内嵌）+ 会话日志空态。
        bool empty_ok = w.docks() != nullptr && w.docks()->files != nullptr
                        && w.docks()->hist != nullptr && w.docks()->sftp != nullptr;
        if (empty_ok) {
            // 面板头换成自绘 PanelHeader 后，dock 里的 QLabel 不止一个（色块/标题/空态），
            // 故按 objectName 定位空态文案，不再取 "first label"。
            QLabel *empty = w.docks()->hist->findChild<QLabel *>(QLatin1String("mutedHint"));
            empty_ok &= empty != nullptr &&
                        empty->text() == i18n::tr("没有可用的会话日志");
            // SFTP 浏览器本地栏即扫（用户目录）+ 传输队列空。
            empty_ok &= w.docks()->sftp->transfer_rows() == 0;
        }
        std::printf("[cpp] C3-d 文件管理器(SFTP)+空态: %s\n", empty_ok ? "通过" : "失败");
        ok &= empty_ok;

        // 5) dock 菜单切换：view:toggle-left-panel 触发 → 连接 dock 隐藏/复原。
        QAction *a_left = find_action("view:toggle-left-panel");
        bool dock_ok = a_left != nullptr && w.docks() != nullptr
                       && w.docks()->conn != nullptr;
        if (dock_ok) {
            const bool was = w.docks()->conn->isVisible();
            a_left->trigger();
            const bool hidden = !w.docks()->conn->isVisible();
            a_left->trigger();
            dock_ok = hidden && w.docks()->conn->isVisible() == was;
        }
        std::printf("[cpp] C3-e dock 切换: %s\n", dock_ok ? "通过" : "失败");
        ok &= dock_ok;

        // 5b) 右键菜单度量（WT：行高 27）。
        {
            QMenu probe;
            chrome::style_menu(&probe);
            QAction *i1 = chrome::proxy_action(&probe, nullptr, QStringLiteral("copy"),
                                               QStringLiteral("复制(C)\tShift+Del"));
            QAction *i2 = chrome::proxy_action(&probe, nullptr, QStringLiteral("paste"),
                                               QStringLiteral("粘贴(P)\tShift+Ins"));
            probe.addSeparator();
            QAction *i3 = chrome::proxy_action(&probe, nullptr, QStringLiteral("search"),
                                               QStringLiteral("查找…(F)\tCtrl+Shift+F"));
            const QSize hint = probe.sizeHint(); // 先触发 actionRects 计算
            const int h1 = probe.actionGeometry(i1).height();
            const int h2 = probe.actionGeometry(i2).height();
            const int h3 = probe.actionGeometry(i3).height();
            const bool menu_ok = h1 == chrome::metrics::kMenuItemHeight && h2 == h1 && h3 == h1;
            std::printf("[cpp] C3-g 菜单行高=%d/%d/%d 目标=%d 菜单=%dx%d: %s\n", h1, h2, h3,
                        chrome::metrics::kMenuItemHeight, hint.width(), hint.height(),
                        menu_ok ? "通过" : "失败");
            ok &= menu_ok;

        }

        inject_poll.start(100);
    });

    // 6) 发送栏功能链：等提示符（6s 兜底）→ 注入 → 发送 → 回显上屏（真实 PTY）。
    QObject::connect(&inject_poll, &QTimer::timeout, [&] {
        waited_ms += 100;
        const rust::String rs = term_screen_text(w.page_id(0));
        const std::string text(rs.data(), rs.size());
        if (!selftest_shell::ready(text) && waited_ms < 6000) {
            return;
        }
        inject_poll.stop();
        if (SendBar *bar = w.page_sendbar(0)) {
#ifdef Q_OS_WIN
            bar->input()->setText(QStringLiteral("Write-Output C3SEND"));
#else
            bar->input()->setText(QStringLiteral("printf '%s%s\\n' 'C3' 'SEND'"));
#endif
            bar->send();
        }
        QTimer::singleShot(1000, [&] {
            const rust::String rs2 = term_screen_text(w.page_id(0));
            const std::string text2(rs2.data(), rs2.size());
            const uint32_t sent =
                w.page_sendbar(0) != nullptr ? w.page_sendbar(0)->sent_count() : 0;
            const bool sent_ok =
                text2.find("C3SEND") != std::string::npos && sent == 1;
            ok &= sent_ok;
            std::printf("[cpp] C3-f 发送栏链路: %s（sent=%u 回显=%s）\n",
                        sent_ok ? "通过" : "失败", sent,
                        text2.find("C3SEND") != std::string::npos ? "命中" : "未见");

            const std::string png_path(png_out.data(), png_out.size());
            if (!png_path.empty()) {
                const QPixmap shot = w.grab();
                const bool saved = shot.save(QString::fromStdString(png_path));
                std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                            saved ? "成功" : "失败", shot.width(), shot.height());
                ok = ok && saved;
            }
            verdict = ok ? 0 : 1;
            std::printf("[cpp] Phase C ③ 自检: %s\n", verdict == 0 ? "通过" : "失败");
            app.exit(verdict);
        });
    });

    return app.exec();
}

/// Phase C ④ 自检：会话树+表单+快速连接。store 经 sess_test_store_dir 重定向
/// 到临时目录（hermetic——绝不读写用户 sessions.json）。
int qt_boot_c4(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c4";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    // hermetic store：临时目录先清空再重定向（上轮自检残留会让行数断言漂移）。
    const QString temp_dir =
        QDir::temp().filePath(QStringLiteral("tideterm-c4-store"));
    QDir(temp_dir).removeRecursively();
    QDir().mkpath(temp_dir);
    sess_test_store_dir(rust::String(temp_dir.toUtf8().toStdString()));

    MainWindow w;
    w.show();

    QTimer inject_poll;
    int waited_ms = 0;
    int verdict = 1;
    bool ok = true;
    uint32_t local_tab = UINT32_MAX;

    QTimer::singleShot(100, [&] {
        QString rows_debug;

        // 1) 表单构建：五协议页 + SSH 默认页。
        bool form_ok = true;
        {
            SessCfg blank = sess_form_get(rust::String(std::string()), 1);
            blank.ssh_host = rust::String("10.0.0.9");
            blank.name = rust::String("prod-db");
            blank.group_path = rust::String("生产 > 数据库");
            const rust::String id_a = sess_save(blank);
            form_ok &= !id_a.empty();
            SessCfg local = sess_form_get(rust::String(std::string()), 0);
            local.name = rust::String("local-dev");
            const rust::String id_b = sess_save(local);
            form_ok &= !id_b.empty();

            // 编辑回填：sess_form_get(id) → 表单 widgets 显示保存值。
            SessCfg back = sess_form_get(id_a, 0);
            SessionForm form(&w, back);
            form_ok &= form.protocol_pages() == 5;
            form_ok &= form.current_protocol_label() == QStringLiteral("SSH");
            form_ok &= form.ssh_host_text() == QStringLiteral("10.0.0.9");
            form_ok &= QString::fromUtf8(back.name.data(), int(back.name.size()))
                       == QStringLiteral("prod-db");
            form_ok &= QString::fromUtf8(back.group_path.data(),
                                         int(back.group_path.size()))
                       == QStringLiteral("生产 > 数据库");
            // 密码永不回填（凭据 skip_serializing）。
            form_ok &= back.ssh_password.empty();
            // Display order differs from persisted kind: Tcp=4, Serial=3.
            auto *protocols = form.findChild<QTabBar *>(QStringLiteral("sessionProtocols"));
            const uint8_t kinds[] = {0, 1, 2, 4, 3};
            form_ok &= protocols != nullptr;
            if (protocols) for (int index = 0; index < 5; ++index) {
                protocols->setCurrentIndex(index);
                SessCfg result;
                form.collect(&result);
                form_ok &= result.kind == kinds[index] && result.name == back.name &&
                    result.ssh_host == back.ssh_host && result.group_path == back.group_path;
            }
        }
        form_ok &= check_session_options(&w);
        std::printf("[cpp] C4-a 表单构建+回填+真实开关与默认设置: %s\n", form_ok ? "通过" : "失败");
        ok &= form_ok;

        // 1b) 凭据库（vault 域桥）：记住密码标志往返 + 设置/存取/解锁/清理。
        bool vault_ok = true;
        {
            const QString vault_dir =
                QDir::temp().filePath(QStringLiteral("tideterm-c4-vault"));
            QDir(vault_dir).removeRecursively();
            QDir().mkpath(vault_dir);
            vault_test_dir(rust::String(vault_dir.toUtf8().toStdString()));

            vault_ok &= !vault_is_set();
            // 设置（空返回 = 成功）后即解锁，可直接存取。
            vault_ok &= vault_setup(rust::String("c4-master-pass-123")).empty();
            // 记住密码标志随会话落 sessions.json，密码本体不落盘。
            SessCfg remembered = sess_form_get(rust::String(std::string()), 1);
            remembered.name = rust::String("vault-probe");
            remembered.ssh_host = rust::String("10.0.0.9");
            remembered.ssh_remember = true;
            remembered.ssh_password = rust::String("c4-secret");
            const rust::String vid = sess_save(remembered);
            vault_ok &= !vid.empty();
            const SessCfg back = sess_form_get(vid, 1);
            vault_ok &= back.ssh_remember;
            vault_ok &= back.ssh_password.empty();
            // 加密存取往返 + has/remove（桥面语义操作；键格式在 Rust 侧内部）。
            vault_ok &= vault_store_password(vid, rust::String("c4-secret")).empty();
            const VaultLoad loaded = vault_load_password(vid);
            vault_ok &= loaded.ok
                && QString::fromUtf8(loaded.secret.data(), int(loaded.secret.size()))
                       == QStringLiteral("c4-secret");
            vault_ok &= vault_has_credentials(vid)
                && !vault_has_credentials(rust::String("c4-nope"));
            // 解锁流：锁定 → 错密码被拒（1 次远低于节流阈值）→ 正密码解锁。
            vault_lock();
            vault_ok &= !vault_is_unlocked();
            vault_ok &= !vault_unlock(rust::String("wrong-passphrase")).empty();
            vault_ok &= vault_unlock(rust::String("c4-master-pass-123")).empty();
            // 会话级清理（无需解锁）。
            vault_ok &= vault_forget_session(vid);
            vault_ok &= !vault_has_credentials(vid);
            vault_ok &= sess_delete(vid);
            vault_test_reset();
        }
        std::printf("[cpp] C4-b2 凭据库: %s\n", vault_ok ? "通过" : "失败");
        ok &= vault_ok;

        // 2) 连接树 CRUD：保存(A=ssh 分组 生产>数据库, B=local) → 4 行 DFS。
        bool crud_ok = true;
        {
            rust::Vec<SessRow> rows = sess_tree_rows();
            rows_debug = QStringLiteral("%1 行").arg(rows.size());
            crud_ok &= rows.size() == 4;
            if (rows.size() == 4) {
                const auto label = [](const SessRow &r) {
                    return QString::fromUtf8(r.label.data(), int(r.label.size()));
                };
                crud_ok &= rows[0].depth == 0 && label(rows[0]) == QStringLiteral("生产");
                crud_ok &= rows[1].depth == 1 && label(rows[1]) == QStringLiteral("数据库");
                crud_ok &= rows[2].depth == 2 && label(rows[2]) == QStringLiteral("prod-db");
                crud_ok &= rows[3].depth == 0 && label(rows[3]) == QStringLiteral("local-dev");
                crud_ok &= rows[2].kind == 1 && rows[0].kind == 0;
            }
        }
        // 2b) 收藏/重命名/排序/删除（直接经桥 CRUD，id 从树行取）。
        {
            rust::Vec<SessRow> rows = sess_tree_rows();
            QString id_a;
            for (const SessRow &r : rows) {
                if (r.kind == 1
                    && QString::fromUtf8(r.label.data(), int(r.label.size()))
                           == QStringLiteral("prod-db")) {
                    id_a = QString::fromUtf8(r.id.data(), int(r.id.size()));
                }
            }
            crud_ok &= !id_a.isEmpty();
            if (!id_a.isEmpty()) {
                const rust::String rid(id_a.toUtf8().toStdString());
                crud_ok &= sess_set_favorite(rid, true);
                rows = sess_tree_rows();
                crud_ok &= rows.size() == 6;
                if (rows.size() == 6) {
                    crud_ok &= rows[0].depth == 0
                               && QString::fromUtf8(rows[0].label.data(),
                                                    int(rows[0].label.size()))
                                      == QStringLiteral("收藏的");
                }
                crud_ok &= sess_rename(rid, "prod-db2");
                // 删除收藏会话 → 回到无收藏（B 无分组 + A 已删 → 只剩 B）。
                crud_ok &= sess_delete(rid);
                rows = sess_tree_rows();
                crud_ok &= rows.size() == 1
                           && QString::fromUtf8(rows[0].label.data(),
                                                int(rows[0].label.size()))
                                  == QStringLiteral("local-dev");
            }
        }
        std::printf("[cpp] C4-b 树 CRUD: %s（%s）\n", crud_ok ? "通过" : "失败",
                    rows_debug.toUtf8().constData());
        ok &= crud_ok;

        // 3) 快速连接（本地）→ 真实 tiderm-core PtyInstance + read_loop + drain 路由。
        local_tab = sess_quick_open(0, rust::String(std::string()), 0,
                                    rust::String(std::string()),
                                    rust::String(std::string()));
        const bool quick_ok = local_tab != UINT32_MAX;
        std::printf("[cpp] C4-c 快速连接(本地): %s（tab=%u）\n",
                    quick_ok ? "通过" : "失败", local_tab);
        ok &= quick_ok;
        if (quick_ok) {
            w.add_tab_with(local_tab);
        }
        inject_poll.start(100);
    });

    // 4) 等 PowerShell 提示符（15s 兜底）→ prompt 队列空 + PNG。
    QObject::connect(&inject_poll, &QTimer::timeout, [&] {
        if (local_tab == UINT32_MAX) {
            inject_poll.stop();
            std::printf("[cpp] Phase C ④ 自检: 失败（无本地标签）\n");
            app.exit(1);
            return;
        }
        waited_ms += 100;
        const rust::String rs = term_screen_text(local_tab);
        const std::string text(rs.data(), rs.size());
        // 提示符判定：PS 5.1 的 "PS 路径>" 行尾无空格（tiderm-core auto 解析到
        // Windows PowerShell；hyperpanes 档是 pwsh 7 的 "> "）——按 '>' 出现判。
        if (!selftest_shell::ready(text) && waited_ms < 15000) {
            return;
        }
        inject_poll.stop();
        const bool shell_ok = selftest_shell::ready(text);
        ok &= shell_ok;
        const rust::String st = term_status(local_tab);
        std::printf(
            "[cpp] C4-d 真实本地会话: %s（等 %dms，屏 %zu 字节，status=%.*s）\n",
            shell_ok ? "通过" : "失败", waited_ms, text.size(),
            int(st.size()), st.data());
        if (!shell_ok && text.size() <= 200) {
            std::printf("[cpp] 屏文本: [%s]\n", text.c_str());
        }

        // 5) prompt 队列空（无 host key/kb 悬挂）。
        const PromptInfo p = sess_take_prompt();
        const bool prompt_ok = p.kind == 0;
        std::printf("[cpp] C4-e 交互提示队列: %s\n", prompt_ok ? "通过（空）" : "失败");
        ok &= prompt_ok;

        // 6) 连接树 UI 与 store 一致（hermetic：1 会话 local-dev；泵节流 1s，
        //    此处同步强刷保证时序确定）。
        w.refresh_chrome();
        const bool tree_ui_ok = w.docks() != nullptr && w.docks()->tree_model != nullptr
                                && w.docks()->tree_model->total_nodes() == int(sess_explorer_rows().size());
        std::printf("[cpp] C4-f 连接树 UI: %s\n", tree_ui_ok ? "通过" : "失败");
        ok &= tree_ui_ok;

        const std::string png_path(png_out.data(), png_out.size());
        if (!png_path.empty()) {
            const QPixmap shot = w.grab();
            const bool saved = shot.save(QString::fromStdString(png_path));
            std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                        saved ? "成功" : "失败", shot.width(), shot.height());
            ok = ok && saved;
        }
        sess_test_store_reset();
        verdict = ok ? 0 : 1;
        std::printf("[cpp] Phase C ④ 自检: %s\n", verdict == 0 ? "通过" : "失败");
        app.exit(verdict);
    });

    return app.exec();
}

// Phase C ⑤ 自检（qt_boot_c5）见 chrome/selftest_c1.cpp（行数门禁拆出）。
