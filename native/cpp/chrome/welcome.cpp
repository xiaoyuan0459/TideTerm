#include "cpp/chrome/selftest_settings.h"
#include "cpp/i18n.h"
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/welcome.h"
#include "cpp/chrome/session_vault.h"

#include <QAction>
#include <QApplication>
#include <QDir>
#include <QHBoxLayout>
#include <QLabel>
#include <QPixmap>
#include <QPushButton>
#include <QTabBar>
#include <QTabWidget>
#include <QTimer>
#include <QVBoxLayout>

#include <cstdio>
#include <string>

namespace {
void trigger_action(MainWindow *win, const QByteArray &id) {
    for (QAction *action : win->findChildren<QAction *>()) {
        if (action->property("tid").toByteArray() == id) {
            action->trigger();
            return;
        }
    }
    win->note(i18n::tr("菜单动作不可用：%1").arg(QString::fromUtf8(id)));
}
}

void MainWindow::show_welcome() {
    if (welcome_) return;
    welcome_ = new QWidget(tabs_);
    auto *layout = new QVBoxLayout(welcome_);
    layout->setContentsMargins(32, 24, 32, 24);
    layout->addStretch(2);
    auto *title = new QLabel(QStringLiteral("TideTerm"), welcome_);
    title->setAlignment(Qt::AlignCenter);
    title->setStyleSheet(QStringLiteral("font-size: 28px; font-weight: 600;"));
    layout->addWidget(title);
    auto *hint = new QLabel(i18n::tr("选择一个会话开始"), welcome_);
    hint->setAlignment(Qt::AlignCenter);
    layout->addWidget(hint);
    layout->addSpacing(22);
    auto *row = new QHBoxLayout;
    row->addStretch(1);
    auto *local = new QPushButton(i18n::tr("本地终端"), welcome_);
    auto *quick = new QPushButton(i18n::tr("快速连接…"), welcome_);
    auto *open = new QPushButton(i18n::tr("打开会话…"), welcome_);
    row->addWidget(local);
    row->addWidget(quick);
    row->addWidget(open);
    row->addStretch(1);
    layout->addLayout(row);
    layout->addSpacing(8);
    auto *new_session = new QPushButton(i18n::tr("新建并保存会话…"), welcome_);
    new_session->setFlat(true);
    layout->addWidget(new_session, 0, Qt::AlignHCenter);
    layout->addStretch(3);
    connect(local, &QPushButton::clicked, this, [this] { add_tab(); });
    connect(quick, &QPushButton::clicked, this,
            [this] { trigger_action(this, "panel:quick"); });
    connect(open, &QPushButton::clicked, this,
            [this] { trigger_action(this, "session:open"); });
    connect(new_session, &QPushButton::clicked, this,
            [this] { trigger_action(this, "session:new"); });
    tabs_->addTab(welcome_, i18n::tr("开始"));
    tabs_->tabBar()->hide();
}

void MainWindow::dismiss_welcome() {
    if (!welcome_) return;
    tabs_->removeTab(tabs_->indexOf(welcome_));
    welcome_->deleteLater();
    welcome_ = nullptr;
    tabs_->tabBar()->show();
}

bool restore_saved_session(MainWindow *window, const QStringList &ids) {
    for (const QString &id : ids) {
        const QByteArray utf8 = id.toUtf8();
        const uint32_t tab = chrome::sess_open_with_vault(
            window, rust::String(utf8.constData(), utf8.size()));
        if (tab == chrome::kVaultLocked) {
            // 用户取消解锁：跳过该会话继续尝试其余候选。
            continue;
        }
        if (tab != UINT32_MAX) {
            window->add_tab_with(tab);
            return true;
        }
    }
    return false;
}

int qt_boot_welcome(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-welcome";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();
    MainWindow w(false);
    w.show();
    QTimer::singleShot(0, [&] {
        bool ok = w.page_count() == 0 && term_tab_count() == 0;
        std::printf("[cpp] Welcome 初始 pages=%d terms=%u\n", w.page_count(), term_tab_count());
        QPushButton *local = nullptr;
        for (QPushButton *button : w.findChildren<QPushButton *>()) {
            if (button->text() == i18n::tr("本地终端")) local = button;
        }
        ok &= local != nullptr;
        const std::string path(png_out.data(), png_out.size());
        if (!path.empty()) ok &= w.grab().save(QString::fromStdString(path));
        if (local) local->click();
        std::printf("[cpp] Welcome 打开 pages=%d terms=%u\n", w.page_count(), term_tab_count());
        ok &= w.page_count() == 1 && term_tab_count() == 1;
        w.close_tab(0);
        std::printf("[cpp] Welcome 关闭 pages=%d terms=%u\n", w.page_count(), term_tab_count());
        ok &= w.page_count() == 0 && term_tab_count() == 0;
        const QString temp_root = QDir(QDir::tempPath()).absolutePath();
        const QString store_path = QDir(temp_root).filePath(
            QStringLiteral("tideterm-welcome-%1").arg(QCoreApplication::applicationPid()));
        const bool safe_path = QDir(store_path).absolutePath().startsWith(temp_root + '/');
        const bool store_ready = safe_path && QDir().mkpath(store_path);
        ok &= store_ready;
        if (store_ready) {
            sess_test_store_dir(rust::String(store_path.toUtf8().toStdString()));
            SessCfg cfg = sess_form_get(rust::String(""), 0);
            cfg.name = rust::String("restore-check");
            const rust::String id = sess_save(cfg);
            ok &= !id.empty();
            const QString valid = QString::fromUtf8(id.data(), int(id.size()));
            const bool restored = restore_saved_session(&w, {QStringLiteral("missing-id"), valid});
            std::printf("[cpp] Restore id=%s open=%d pages=%d terms=%u\n",
                        valid.toUtf8().constData(), restored, w.page_count(), term_tab_count());
            ok &= restored && w.page_count() == 1 && term_tab_count() == 1;
            if (w.page_count() == 1) w.close_tab(0);
            ok &= w.page_count() == 0 && term_tab_count() == 0;
            sess_test_store_reset();
            QDir(store_path).removeRecursively();
        }
        std::printf("[cpp] Welcome 空启动/打开/关闭: %s\n", ok ? "通过" : "失败");
        app.exit(ok ? 0 : 1);
    });
    return app.exec();
}
