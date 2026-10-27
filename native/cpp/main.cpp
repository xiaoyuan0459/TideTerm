#include "cpp/chrome/selftest_settings.h"
// native C++ 入口面：Qt 版本上报 + 脚手架假引擎自检 + Phase C ①② 自检 + G3 编排。
// Phase C ③④（chrome 骨架 / 会话树+表单+快连）自检在 cpp/chrome/selftest_chrome.cpp。
// MainWindow（标签/分割/菜单主窗口）在 cpp/main_window.{h,cpp}；PtyView 编排器
// （pty/g2/交互）在 cpp/terminal/terminal_view.cpp。

#include "cpp/bridge_shim.h"
#include "cpp/bridge_spike.h"
#include "cpp/chrome/docks.h"
#include "cpp/chrome/app_font.h"
#include "cpp/chrome/app_icon.h"
#include "cpp/chrome/ui_capture.h"
#include "cpp/chrome/mem_snapshot.h"
#include "cpp/chrome/pod_tree_model.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/chrome/session_banner.h"
#include "cpp/chrome/welcome.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/sftp/sftp_view.h"
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/performance_probe.h"

#include <QApplication>
#include <QDir>
#include <QDockWidget>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QPixmapCache>
#include <QPainter>
#include <QPixmap>
#include <QSettings>
#include <QSplitter>
#include <QTimer>
#include <QToolBar>
#include <QTreeWidget>
#include <QTreeView>
#include <QListView>
#include <QFile>
#include <QTemporaryDir>
#include <QVBoxLayout>
#include <QWidget>

#include <cstdio>
#include <functional>
#include <memory>
#include <string>
#include <vector>

rust::String qt_version() {
    return rust::String(qVersion());
}

// 桥拆域 spike（ADR-014 §Decision 3）：调用第二桥（bridge/scaffold.rs，文件模块
// 挂载 + pub use 重导出）的 Rust 实现——cxx 把 extern "Rust" 声明在全局作用域
// （各生成头皆然），跨桥函数名必须全局唯一。
uint32_t qt_spike_passthrough(uint32_t v) {
    return spike_rust_id() + v;
}

int qt_boot(bool selftest, uint32_t frames, rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    i18n::install_from_env();

    // 脚手架假引擎视图（--selftest 保留；帧由 Rust frame_for 驱动）。
    class FakeView : public QWidget {
    public:
        explicit FakeView(uint32_t total) : total_(total) { setFixedSize(640, 400); }
        uint32_t frames_painted() const { return painted_; }

    protected:
        void paintEvent(QPaintEvent *) override {
            if (painted_ >= total_) return;
            RawSurface surface = frame_for(painted_);
            QImage image(reinterpret_cast<const uchar *>(surface.ptr),
                         static_cast<int>(surface.width), static_cast<int>(surface.height),
                         static_cast<int>(surface.stride), QImage::Format_RGBA8888);
            QPainter painter(this);
            painter.drawImage(0, 0, image);
            painter.setPen(Qt::white);
            painter.drawText(12, 24, QStringLiteral("frame %1 / %2").arg(painted_).arg(total_));
            ++painted_;
            on_frame_painted(painted_);
        }
        void keyPressEvent(QKeyEvent *e) override { on_key(e->key()); }

    private:
        uint32_t total_;
        uint32_t painted_ = 0;
    };

    FakeView view(frames);
    view.show();
    if (!selftest) {
        return app.exec();
    }
    // render() drives paintEvent even before an offscreen window is exposed.
    QImage offscreen(view.size(), QImage::Format_RGBA8888);
    for (uint32_t i = 0; i < frames; ++i) {
        view.render(&offscreen);
    }
    const uint32_t painted = view.frames_painted();
    const std::string png_path(png_out.data(), png_out.size());
    if (!png_path.empty()) {
        const QPixmap shot = view.grab();
        const bool saved = shot.save(QString::fromStdString(png_path));
        std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                    saved ? "成功" : "失败", shot.width(), shot.height());
    }
    std::printf("[cpp] 已绘帧数: %u / %u\n", painted, frames);
    return painted == frames ? 0 : 1;
}

int qt_window(rust::Str profile) {
    static int argc = 1;
    static char arg0[] = "native";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    if (!profile.empty()) {
        const auto directory = QString::fromUtf8(profile.data(), int(profile.size()));
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory);
        QSettings::setPath(QSettings::IniFormat, QSettings::SystemScope, directory);
    }
    chrome::install_app_icon(app);
    // Qt 默认给 QPixmapCache 留 10 MiB「重量」上限；本应用的图标/像素图用量远低于此
    // （终端像素由 Rust 侧软件渲染持有，不进这个缓存），收紧到 2 MiB 挡住偶发缓存的
    // 高水位驻留（路线图 §8 动态内存治理 2026-09-28）。
    QPixmapCache::setCacheLimit(2048);
    // 语言：偏好显式选择优先；偏好为空（跟随系统）时回落到系统 locale。
    const QString pref = QString::fromUtf8(prefs_get().locale.c_str());
    if (!pref.isEmpty()) {
        i18n::install(pref);
    } else {
        i18n::install_from_env();
    }
    MainWindow w(false);
    w.show();
    bool restored = false;
    if (prefs_get().restore_last_session) {
        if (!workspace_list().empty()) restored = w.restore_previous_workspace();
        else {
            w.add_tab();
            QSettings settings(QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
            const QStringList ids = settings.value(QStringLiteral("recentSessions")).toStringList();
            restore_saved_session(&w, ids);
        }
    }
    // 用户裁决（2026-09-25）：启动不停在欢迎页——无最近会话/恢复失败时直接开本机 shell。
    // 欢迎页只在用户自己关掉全部标签后出现（MainWindow::close_tab 的空栈分支）。
    if (w.page_count() == 0) w.add_tab();
    if (!restored) w.focus_session(w.page_id(0));
    chrome::capture_ui_if_requested(&w);
    // 内存构成取证（`TIDETERM_MEM_SNAPSHOT=1` 才开；采样时刻与 measure-startup.ps1 一致）。
    if (qEnvironmentVariableIsSet("TIDETERM_MEM_SNAPSHOT")) {
        QTimer::singleShot(6000, [] { chrome::dump_mem_snapshot("window"); });
    }
    return app.exec();
}

int qt_memory_probe(uint8_t stage) {
    static int argc = 1;
    static char arg0[] = "native-memory-probe";
    static char *argv[] = {arg0, nullptr};
    performance_probe::Application app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    if (stage == 1 || stage == 2 || stage == 5 || stage == 6 || stage == 7)
        chrome::init_app_font();
    QWidget minimal;
    std::unique_ptr<MainWindow> window;
    std::unique_ptr<PtyView> solo; // stage 13：不带任何 MainWindow 页壳的孤立窗格
    // stage 78：8 个孤立窗格平铺成 4×2，量"每格真绘制"的斜率（避开分割树把格子挤扁）。
    std::vector<std::unique_ptr<PtyView>> grid;
    // stage 79：8 格灌真实滚动历史 → 关 7 格 → 观察动态内存自然回落（回收链路口径）。
    bool recycled = false;
    if (stage == 3 || stage == 4 || stage == 8 || stage == 9 || stage == 10 || stage == 12
        || stage == 15 || stage == 76 || stage == 77 || stage == 79) {
        window = std::make_unique<MainWindow>(stage == 4 || stage == 9 || stage == 10
                                             || stage == 12 || stage == 15 || stage == 76
                                             || stage == 77 || stage == 79);
        if (stage == 9) {
            window->pane_view(0, 0)->setUpdatesEnabled(false);
        } else if (stage == 10) {
            window->setProperty("memoryProbeBlankPty", true);
        } else if (stage == 12) {
            // 会话泵一次都不跑：把「泵里的桥调用 + 每秒 chrome/docks 刷新」从常驻成本里摘掉。
            window->halt_pump_for_probe();
        } else if (stage == 15) {
            // 页壳在、窗格不可见：隔离"窗格自己的绘制/后备存储"与"整页常驻成本"。
            window->halt_pump_for_probe();
            window->pane_view(0, 0)->hide();
        } else if (stage == 76 || stage == 77 || stage == 79) {
            // 8 窗格口径：同一页 split 到 8 格，每格灌同一行中文，泵全程不跑。
            // 76 = 每格 grab() 强制真绘制（生产路径）；77 = 每格 hide()（只留引擎与页壳）。
            // 79 = 每格再灌 ~4000 行真实历史进压缩层，量"关格后动态内存是否回落"。
            // 与 stage 12（单格、泵停）对照即得"每多一格花多少"。
            window->halt_pump_for_probe();
            for (int i = 0; i < 7; ++i) {
                // 交替轴向：同向会让分割树左倾，最深那几格被挤成 2 列，绘制口径失真。
                window->split_active(i % 2 ? Qt::Vertical : Qt::Horizontal);
            }
            const std::string line = "中文测试 abc 1234\n";
            for (int j = 0; PtyView *v = window->pane_view(0, j); ++j) {
                term_feed_inject(v->tab(), rust::Slice<const uint8_t>(
                    reinterpret_cast<const uint8_t *>(line.data()), line.size()));
                if (stage == 77) v->hide();
            }
            if (stage == 79) {
                // 每格 ~4000 行 × ~76 B（UTF-8 中文行），驱动压缩层真积累（预算内远端）。
                std::string bulk;
                bulk.reserve(line.size() * 4000);
                for (int r = 0; r < 4000; ++r) bulk += line;
                for (int j = 0; PtyView *v = window->pane_view(0, j); ++j) {
                    term_feed_inject(v->tab(), rust::Slice<const uint8_t>(
                        reinterpret_cast<const uint8_t *>(bulk.data()), bulk.size()));
                }
            }
        }
        window->show();
    } else {
        minimal.resize(1200, 800);
        minimal.show();
    }
    performance_probe::install_load(window.get(), stage);
    uint32_t tab = stage == 2 || stage == 5 || stage == 6 || stage == 7 || stage == 8
        ? term_new_tab() : UINT32_MAX;
    if (stage == 13 || (stage >= 73 && stage <= 75)) {
        // 孤立窗格：同一个 PtyView + 同尺寸视口 + 同一个引擎会话，但没有 MainWindow 的
        // 页壳（横幅/发送栏/分割器/dock/菜单）。与 stage 12 对照即可把 41 MB 归到一侧。
        // 73~75 = 中文进屏要多少钱的三档口径（2026-09-19 实测，6 s PrivateMB）：
        // 73 纯 ASCII 下限 11.9 ｜ 74 中文走单族 CJK 字面（生产路径）12.0
        // ｜ 75 改前那条 Rust 软光栅 20.5。74 若涨回 ~53，就是有人把回退链引回来了。
        tab = term_new_tab();
        const std::string feed =
            stage == 73 ? "ASCII abcdef 1234\nABCDEFGHIJKL\n" : "中文测试 abc 1234\n全角ＡＢＣ尾\n";
        term_feed_inject(tab, rust::Slice<const uint8_t>(
            reinterpret_cast<const uint8_t *>(feed.data()), feed.size()));
        solo = std::make_unique<PtyView>(tab);
        solo->setParent(&minimal);
        solo->setGeometry(0, 0, 1200, 800);
        solo->show(); // 与真实窗格一样：可见 + 会收到 paintEvent
    }
    if (stage == 78) {
        const std::string line = "中文测试 abc 1234\n";
        for (int i = 0; i < 8; ++i) {
            const uint32_t id = term_new_tab();
            term_feed_inject(id, rust::Slice<const uint8_t>(
                reinterpret_cast<const uint8_t *>(line.data()), line.size()));
            auto v = std::make_unique<PtyView>(id);
            v->setParent(&minimal);
            v->setGeometry((i % 4) * 300, (i / 4) * 400, 300, 400);
            v->show();
            grid.push_back(std::move(v));
        }
    }
    if (stage == 14) {
        // 孤立页壳：与终端页同样的三件套（横幅/分割器/发送栏），但**不放窗格**、
        // 不建 MainWindow —— 把"页壳控件本身"的成本与"窗格挂进主窗"的成本分开。
        auto *root = new QWidget(&minimal);
        auto *lay = new QVBoxLayout(root);
        lay->setContentsMargins(0, 0, 0, 0);
        lay->setSpacing(0);
        auto *split = new QSplitter(Qt::Vertical, root);
        lay->addWidget(new SessionBanner(root));
        lay->addWidget(split, 1);
        lay->addWidget(new SendBar(root));
        root->setGeometry(0, 0, 1200, 800);
        root->show();
    }
    if (stage == 6 || stage == 7) {
        const std::string sample = stage == 6 ? "中文测试" : "ASCII test";
        term_feed_inject(tab, rust::Slice<const uint8_t>(
            reinterpret_cast<const uint8_t *>(sample.data()), sample.size()));
        term_frame(tab);
    }
    QTimer render_pump;
    if (stage == 5 || stage == 8) {
        QObject::connect(&render_pump, &QTimer::timeout, [&] {
            term_poll(tab);
            if (stage == 5) term_frame(tab);
        });
        render_pump.start(20);
    }
    QTimer feed_repaint;
    if ((stage >= 73 && stage <= 76) || stage == 78) {
        // 隐藏窗口收不到 expose（实测 paintEvent 计时 0 ns）⇒ 用 grab() 强制走一遍真实绘制，
        // 否则"便宜"只证明了我们没画。
        QObject::connect(&feed_repaint, &QTimer::timeout, [&] {
            if (stage == 78) {
                for (auto &v : grid) {
                    term_poll(v->tab());
                    v->grab();
                }
                return;
            }
            if (stage == 76) {
                for (int j = 0; PtyView *v = window->pane_view(0, j); ++j) {
                    term_poll(v->tab());
                    v->grab();
                }
                return;
            }
            term_poll(tab);
            if (stage == 75)
                term_frame(tab); // 改前那条路：Rust 侧 swash 自己光栅化整屏
            else if (solo)
                solo->grab();
        });
        feed_repaint.start(200);
    }
    // 第 6 秒（采样口径的读数时刻）打一行成分：Qt 侧（可见性/子控件数）+ Rust 侧分量。
    QTimer::singleShot(6000, [&] {
        const int shown = window ? int(window->isVisible()) : -1;
        const int kids = window ? int(window->findChildren<QWidget *>().size()) : 0;
        const rust::String rep = term_mem_report();
        const QString text = QString::fromUtf8(rep.data(), int(rep.size()));
        std::printf("[probe] 主窗可见=%d 子控件=%d 全控件=%d 独立窗格=%d | %s\n", shown, kids,
                    int(QApplication::allWidgets().size()), int(minimal.isVisible()),
                    text.toUtf8().constData());
        if (qEnvironmentVariableIsSet("TIDETERM_MEM_SNAPSHOT")) {
            char tag[32];
            std::snprintf(tag, sizeof(tag), "stage=%u", unsigned(stage));
            chrome::dump_mem_snapshot(tag);
        }
        if (stage == 76 || stage == 77 || stage == 78 || stage == 79) {
            int panes = 0;
            qint64 ns = 0;
            if (stage == 78) {
                for (auto &v : grid) {
                    ++panes;
                    ns += v->paint_ns();
                }
            } else {
                for (int j = 0; PtyView *v = window->pane_view(0, j); ++j) {
                    ++panes;
                    ns += v->paint_ns();
                }
            }
            std::printf("[probe%2d] 窗格=%d 累计绘制=%.0f ms\n", stage, panes, double(ns) / 1e6);
        }
        if (stage == 79 && !recycled) {
            // 动态内存回收口径：关掉 7 格（保留 pane 0），1.5s 后再看一次成分——
            // Rust 侧 drop（TABS erase → 压缩层释放）之后，进程私有提交/堆存活有没有
            // 真回落；HeapCompact 实验在此基础上做（此探针先量自然回落基线）。
            recycled = true;
            for (int j = 7; j >= 1; --j) {
                if (PtyView *v = window->pane_view(0, j)) window->close_pane(v);
            }
            std::printf("[probe79] 已关 7 格，剩余=%zu\n", window->pane_ids(0).size());
            QTimer::singleShot(1500, [&] {
                chrome::dump_mem_snapshot("stage=79-closed");
                const rust::String rep2 = term_mem_report();
                std::printf("[probe79] 关格后 %.*s\n", int(rep2.size()), rep2.data());
            });
        }
        if (stage >= 73 && stage <= 75 && tab != UINT32_MAX) {
            // 顺带证明"屏上真有字、窗格真画过"，否则便宜只说明我们什么都没画。
            const rust::Vec<uint32_t> rec = term_spans(tab);
            size_t texts = 0, cjk = 0;
            for (size_t i = 0; i + 5 < rec.size(); i += 6) {
                if (rec[i] != 1) continue;
                ++texts;
                if (rec[i + 4] >= 0x2E80u) ++cjk;  // 码位在字记录的第 5 个字
            }
            std::printf("[probe%2d] 记录=%zu 文本=%zu 中文=%zu 窗格绘制=%lldns\n", stage, rec.size(),
                        texts, cjk, (long long)(solo ? solo->paint_ns() : -1));
        }
    });
    // Extend the same diagnostic window for settled resource samples; normal
    // application lifetime and the default eight-second probe are unchanged.
    bool duration_ok = false;
    const int duration = qEnvironmentVariableIntValue("TIDETERM_MEMORY_PROBE_SECONDS", &duration_ok);
    const int seconds = duration_ok ? qBound(8, duration, 120) : 8;
    // A diagnostic owns its isolated sessions and fixed lifetime. QApplication's
    // Quit event asks normal windows to close, which can open the user's normal
    // confirmation dialog and leave this unattended probe alive indefinitely.
    QTimer::singleShot(seconds * 1000, &app, [] { QCoreApplication::exit(0); });
    app.arm(window ? static_cast<QWidget *>(window.get()) : &minimal);
    const int result = app.exec();
    app.report();
    if (tab != UINT32_MAX) term_close_tab(tab);
    return result;
}

// Phase C ① 自检（qt_boot_c1）见 chrome/selftest_c1.cpp（行数门禁拆出）。

int qt_boot_g3(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-g3";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    QTreeWidget *tree = nullptr;
    QToolBar *toolbar = nullptr;
    QWidget *w = build_sftp_browser(&tree, &toolbar);
    w->show();
    w->repaint();

    bool ok = tree != nullptr && toolbar != nullptr;
    ok = ok && tree->topLevelItemCount() == 24;
    ok = ok && tree->dragEnabled() && tree->acceptDrops();
    // File filtering was removed at the user's request. Its retired regression
    // is not a requirement; keep directory and drag/drop checks here and in C5.
    std::printf("[cpp] G3 结构断言: %s（行=%d 工具条动作=%d 拖拽=%d）\n",
                ok ? "通过" : "失败", tree ? tree->topLevelItemCount() : -1,
                toolbar ? int(toolbar->actions().size()) : -1,
                tree ? int(tree->dragEnabled() && tree->acceptDrops()) : -1);

    const std::string png_path(png_out.data(), png_out.size());
    if (!png_path.empty()) {
        const QPixmap shot = w->grab();
        const bool saved = shot.save(QString::fromStdString(png_path));
        std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                    saved ? "成功" : "失败", shot.width(), shot.height());
        ok = ok && saved;
    }
    app.exit(ok ? 0 : 1);
    return ok ? 0 : 1;
}
