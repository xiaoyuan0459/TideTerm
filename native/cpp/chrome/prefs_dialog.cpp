#include "cpp/chrome/selftest_settings.h"
// PrefsDialog 实现（Phase C ⑦）。

#include "cpp/i18n.h"
#include "cpp/chrome/prefs_dialog.h"
#include "cpp/chrome/tab_close.h"
#include "cpp/chrome/selftest_close_menu.h"
#include "cpp/chrome/ui_theme.h"

#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QPixmap>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>
#include <QTabWidget>
#include <QVBoxLayout>
#include <QTimer>
#include <QThread>
#include <QLineEdit>
#include <QAction>
#include "cpp/terminal/terminal_view.h"
#include "cpp/chrome/menu_style.h"
#include <vector>

namespace {

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 光标样式选择行：三个互斥单选（块/下划线/竖线，WindTerm 语义）。
/// 选中态写入 *style；返回容器（owner 管理生命周期）。
QWidget *cursor_style_row(int *style) {
    auto *row = new QWidget;
    auto *lay = new QHBoxLayout(row);
    lay->setContentsMargins(0, 0, 0, 0);
    // 三按钮同父即互斥（QRadioButton 语义），无需按钮组。
    auto *block = new QRadioButton(i18n::tr("块"), row);
    auto *under = new QRadioButton(i18n::tr("下划线"), row);
    auto *bar = new QRadioButton(i18n::tr("竖线"), row);
    block->setChecked(*style == 0);
    under->setChecked(*style == 1);
    bar->setChecked(*style == 2);
    QObject::connect(block, &QRadioButton::toggled, row,
                     [style, block](bool on) { if (on) *style = 0; });
    QObject::connect(under, &QRadioButton::toggled, row,
                     [style, under](bool on) { if (on) *style = 1; });
    QObject::connect(bar, &QRadioButton::toggled, row,
                     [style, bar](bool on) { if (on) *style = 2; });
    lay->addWidget(block);
    lay->addWidget(under);
    lay->addWidget(bar);
    lay->addStretch(1);
    return row;
}

} // namespace

bool run_prefs_dialog(MainWindow *win, int focus_page) {
    const UiPrefsPod cur = prefs_get();

    QDialog dlg(win);
    dlg.setWindowTitle(i18n::tr("首选项"));
    auto *root = new QVBoxLayout(&dlg);

    auto *tabs = new QTabWidget(&dlg);
    root->addWidget(tabs);

    // ---- 常规：主题 + 恢复会话 ----
    auto *general = new QWidget(&dlg);
    auto *gform = new QFormLayout(general);
    auto *theme = new QComboBox(general);
    const rust::Vec<ThemePod> themes = prefs_themes();
    int current_idx = 0;
    for (size_t i = 0; i < themes.size(); ++i) {
        theme->addItem(q(themes[i].name));
        if (q(themes[i].id) == q(cur.theme)) {
            current_idx = int(i);
        }
    }
    theme->setCurrentIndex(current_idx);
    gform->addRow(i18n::tr("主题"), theme);
    auto *language = new QComboBox(general);
    language->setObjectName(QStringLiteral("uiLanguage"));
    language->addItem(i18n::tr("跟随系统"), QString());
    language->addItem(QStringLiteral("简体中文"), QStringLiteral("zh_CN"));
    language->addItem(QStringLiteral("English"), QStringLiteral("en"));
    language->setCurrentIndex(qMax(0, language->findData(q(cur.locale))));
    gform->addRow(i18n::tr("界面语言（重启后生效）"), language);
    auto *restore = new QCheckBox(i18n::tr("启动时恢复上次会话"), general);
    restore->setChecked(cur.restore_last_session);
    gform->addRow(QString(), restore);
    auto *confirm_close = new QCheckBox(i18n::tr("关闭窗口前确认"), general);
    confirm_close->setObjectName(QStringLiteral("confirmWindowClose"));
    confirm_close->setChecked(chrome::window_close_confirmation_enabled());
    gform->addRow(QString(), confirm_close);
    tabs->addTab(general, i18n::tr("常规"));

    // ---- 终端：光标/行号/回滚预算 ----
    auto *term = new QWidget(&dlg);
    auto *tform = new QFormLayout(term);
    int style = int(cur.cursor_style);
    tform->addRow(cursor_style_row(&style));
    auto *blink = new QCheckBox(i18n::tr("光标闪烁"), term);
    blink->setChecked(cur.cursor_blink);
    tform->addRow(QString(), blink);
    auto *gutter = new QCheckBox(i18n::tr("显示行号"), term);
    gutter->setChecked(cur.line_numbers);
    tform->addRow(QString(), gutter);
    auto *scrollback = new QSpinBox(term);
    // 区间与 Rust 侧 `UiPrefs::SCROLLBACK_MB_MIN/MAX` 同值（越界值在 load 时被钳回）。
    scrollback->setRange(4, 512);
    scrollback->setSuffix(i18n::tr(" MiB/窗格"));
    scrollback->setValue(int(cur.scrollback_mb));
    tform->addRow(i18n::tr("回滚缓冲"), scrollback);
    tabs->addTab(term, i18n::tr("终端"));

    // ---- SSH：超时/host key ----
    auto *ssh = new QWidget(&dlg);
    auto *sform = new QFormLayout(ssh);
    auto *timeout = new QSpinBox(ssh);
    timeout->setRange(3, 120);
    timeout->setSuffix(i18n::tr(" 秒"));
    timeout->setValue(int(cur.ssh_connect_timeout));
    sform->addRow(i18n::tr("连接超时"), timeout);
    auto *hostkey = new QCheckBox(i18n::tr("自动接受未知主机密钥"), ssh);
    hostkey->setChecked(cur.auto_accept_host_key);
    sform->addRow(QString(), hostkey);
    tabs->addTab(ssh, QStringLiteral("SSH"));

    // 菜单三入口定位同一对话框到对应页。
    if (focus_page >= 0 && focus_page < tabs->count()) {
        tabs->setCurrentIndex(focus_page);
    }

    auto *buttons = new QDialogButtonBox(&dlg);
    buttons->addButton(QDialogButtonBox::Ok);
    buttons->addButton(QDialogButtonBox::Cancel);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dlg,
                     [&dlg] { dlg.accept(); });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dlg,
                     [&dlg] { dlg.reject(); });
    root->addWidget(buttons);

    if (dlg.exec() != QDialog::Accepted) {
        return false;
    }

    // 保存（原子写 + 缓存刷新；未知主题 id 由 Rust 侧 normalized 回默认），
    // 然后全局应用主题 QSS（ThemePod 平行数组回查选中 id）。
    UiPrefsPod next;
    next.theme = themes.empty()
                     ? rust::String(cur.theme)
                     : rust::String(themes[size_t(theme->currentIndex())].id);
    next.restore_last_session = restore->isChecked();
    next.cursor_blink = blink->isChecked();
    next.ssh_connect_timeout = uint64_t(timeout->value());
    next.auto_accept_host_key = hostkey->isChecked();
    next.scrollback_mb = uint64_t(scrollback->value());
    next.cursor_style = uint8_t(style);
    next.line_numbers = gutter->isChecked();
    next.locale = rs(language->currentData().toString());
    prefs_set(next);
    if (!chrome::set_window_close_confirmation(confirm_close->isChecked()))
        win->note(i18n::tr("无法保存关闭确认设置，请重试"));
    // 保存校验（F-04 教训）：prefs_set 失败只走 tracing 会被静默吞掉，用户以为
    // 已保存而下次启动回退。落盘/归一化不一致时立即提示，而不是无声丢失。
    const UiPrefsPod saved = prefs_get();
    if (q(saved.theme) != q(next.theme)) {
        win->note(i18n::tr("偏好保存未生效（主题回到 %1），请重试").arg(q(saved.theme)));
    }
    if (q(saved.locale) != q(next.locale) || saved.cursor_blink != next.cursor_blink) {
        win->note(i18n::tr("偏好保存失败，请重试"));
    } else if (q(saved.locale) != q(cur.locale)) {
        win->note(i18n::tr("界面语言将在重启后生效"));
    }
    win->apply_theme();
    return true;
}

// Exercise the real dialog save/cancel path, not only the preference POD.
static bool preferences_dialog_check(MainWindow *win) {
    const UiPrefsPod before = prefs_get();
    UiPrefsPod probe = before;
    probe.locale = rust::String("en"); prefs_set(probe);
    QTimer::singleShot(0, [] {
        auto *dlg = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (dlg) dlg->accept();
    });
    const bool preserved = run_prefs_dialog(win, 0) && q(prefs_get().locale) == QStringLiteral("en");
    QTimer::singleShot(0, [] {
        auto *dlg = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (dlg) {
            auto *language = dlg->findChild<QComboBox *>(QStringLiteral("uiLanguage"));
            if (language) language->setCurrentIndex(language->findData(QStringLiteral("zh_CN")));
            dlg->accept();
        }
    });
    const bool changed = run_prefs_dialog(win, 0) && q(prefs_get().locale) == QStringLiteral("zh_CN");
    QTimer::singleShot(0, [] {
        auto *dlg = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (dlg) {
            auto *language = dlg->findChild<QComboBox *>(QStringLiteral("uiLanguage"));
            if (language) language->setCurrentIndex(0);
            dlg->reject();
        }
    });
    const bool cancelled = !run_prefs_dialog(win, 0) && q(prefs_get().locale) == QStringLiteral("zh_CN");
    prefs_set(before);
    std::printf("[cpp] C7-e dialog locale preservation/change/cancel: %d/%d/%d\n", preserved, changed, cancelled);
    return preserved && changed && cancelled;
}

static bool terminal_actions_check(MainWindow *win) {
    PtyView *view = win->active_view();
    if (!view) return false;
    const uint32_t tab = view->tab();
    term_disconnect_tab(tab); term_poll(tab);
    term_clear(tab, 2);
    auto feed = [tab](const std::string &bytes) { term_feed_inject(tab, rust::Slice<const uint8_t>(
        reinterpret_cast<const uint8_t *>(bytes.data()), bytes.size())); };
    auto trigger = [win](const char *id) {
        QAction *action = chrome::find_menu_action(win->menuBar(), QLatin1String(id));
        if (!action || !action->isEnabled()) return false;
        action->trigger(); return true;
    };
    feed("Cat cat scatter cat\r\n");
    bool ok = q(term_search_set(tab, "cat", 0)).endsWith(QStringLiteral("/4"));
    QTimer::singleShot(0, [] {
        auto *dlg = dynamic_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dlg) return;
        dlg->findChild<QLineEdit *>(QStringLiteral("searchQuery"))->setText(QStringLiteral("cat"));
        dlg->findChild<QCheckBox *>(QStringLiteral("searchCase"))->setChecked(true);
        dlg->findChild<QCheckBox *>(QStringLiteral("searchWord"))->setChecked(true);
        dlg->accept();
    });
    ok &= trigger("search:find");
    ok &= term_search_flags(tab) == 5 && q(term_search_set(tab, "cat", 5)).endsWith(QStringLiteral("/2"));
    ok &= q(term_search_set(tab, "C.t", 3)).endsWith(QStringLiteral("/1"));
    ok &= q(term_search_set(tab, "[", 2)).startsWith(QStringLiteral("无效的正则表达式"));
    ok &= q(term_search_query(tab)) == QStringLiteral("C.t") && term_search_flags(tab) == 3;
    term_clear(tab, 2);
    feed("OLD_HISTORY\r\n");
    for (int i = 0; i < 80; ++i) feed("filler\r\n");
    feed("VISIBLE");
    ok &= trigger("edit:clear-scrollback");
    ok &= q(term_screen_text(tab)).contains(QStringLiteral("VISIBLE"));
    ok &= q(term_search_set(tab, "OLD_HISTORY", 0)).endsWith(QStringLiteral("/0"));
    for (int i = 0; i < 80; ++i) feed("\r\nHISTORY");
    ok &= trigger("edit:clear-screen");
    ok &= q(term_screen_text(tab)).trimmed().isEmpty();
    ok &= !q(term_search_set(tab, "HISTORY", 0)).endsWith(QStringLiteral("/0"));
    ok &= trigger("edit:clear-all");
    ok &= q(term_search_set(tab, "HISTORY", 0)).endsWith(QStringLiteral("/0"));
    term_set_render_opts(tab, 0, false);
    feed("https://fixture.invalid"); view->grab();
    const Rect caret = term_caret_rect(tab);
    ok &= !term_link_target(tab, caret.x - 8 * caret.w, caret.y + caret.h / 2).empty();
    const UiPrefsPod before = prefs_get();
    UiPrefsPod probe = before; probe.cursor_blink = true; prefs_set(probe);
    term_cursor_tick(tab, false); term_cursor_tick(tab, true);
    const auto on = term_spans(tab);
    QThread::msleep(550); term_cursor_tick(tab, true);
    const auto off = term_spans(tab);
    ok &= std::vector<uint32_t>(on.begin(), on.end()) != std::vector<uint32_t>(off.begin(), off.end());
    probe.cursor_blink = false; prefs_set(probe); term_cursor_tick(tab, true);
    const auto steady = term_spans(tab);
    ok &= std::vector<uint32_t>(on.begin(), on.end()) == std::vector<uint32_t>(steady.begin(), steady.end());
    prefs_set(before);
    std::printf("[cpp] C7-f terminal search/clear/link/cursor: %s\n", ok ? "PASS" : "FAIL");
    return ok;
}

// ---- Phase C ⑦ 自检 ----

/// 偏好+主题自检：主题清单/查找回落 / 控件实际配色与主题切换 / 偏好往返（
/// 未知主题 id 回落默认）/ 菜单首选项入口结构。
int qt_boot_c7(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c7";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    bool ok = true;

    // 1) 主题清单。
    MainWindow w;
    w.show();
    w.apply_theme();
    const bool themes_ok = !prefs_themes().empty();
    std::printf("[cpp] C7-a 主题清单: %s\n", themes_ok ? "通过" : "失败");
    ok &= themes_ok;

    // 2) 偏好往返：set（未知主题 id）→ get 回落默认；再 set 原值还原。
    bool round_ok = true;
    {
        const UiPrefsPod before = prefs_get();
        UiPrefsPod probe;
        probe.theme = rust::String("no-such-theme");
        probe.restore_last_session = true;
        probe.cursor_blink = false;
        probe.ssh_connect_timeout = 42;
        probe.auto_accept_host_key = true;
        probe.scrollback_mb = 64;
        probe.cursor_style = 1;
        probe.line_numbers = true;
        prefs_set(probe);
        const UiPrefsPod back = prefs_get();
        round_ok &= std::string(back.theme) != "no-such-theme"; // normalized 回默认
        round_ok &= back.restore_last_session;
        round_ok &= !back.cursor_blink;
        round_ok &= back.ssh_connect_timeout == 42;
        round_ok &= back.auto_accept_host_key;
        round_ok &= back.scrollback_mb == 64;
        round_ok &= back.cursor_style == 1;
        round_ok &= back.line_numbers;
        prefs_set(before); // 还原用户配置
    }
    std::printf("[cpp] C7-b 偏好往返(未知 id 回落): %s\n", round_ok ? "通过" : "失败");
    ok &= round_ok;

    // 3) 主题切换影响实际控件（含无父弹出件），不依赖 QSS 存在于哪层。
    bool switch_ok = true;
    {
        const UiPrefsPod snapshot = prefs_get();
        QWidget child(&w), popup;
        // Canonical theme colors from native/themes/{windterm,light}.ts（只剩这两套）。
        const struct { const char *id, *bg, *fg; } cases[] = {
            {"windterm", "#1c1c1c", "#bbbbbb"}, {"light", "#f5f5f5", "#1a1a1e"}};
        for (const auto &t : cases) {
            UiPrefsPod p = snapshot;
            p.theme = rust::String(t.id);
            prefs_set(p);
            w.apply_theme();
            for (QWidget *widget : {static_cast<QWidget *>(&w), &child, &popup}) {
                widget->ensurePolished();
                switch_ok &= widget->palette().color(QPalette::Window) == QColor(t.bg);
                switch_ok &= widget->palette().color(QPalette::WindowText) == QColor(t.fg);
                switch_ok &= widget->font().pixelSize() == 13;
            }
            // Q-05 回归：light 的终端底/前景必须跟主题（自绘件读 ui_colors()，
            // 不走 QSS——黑底黑字就是这条链断了）。
            if (std::string_view(t.id) == "light") {
                switch_ok &= chrome::ui_colors().term_bg == QColor("#ffffffff");
                switch_ok &= chrome::ui_colors().term_fg == QColor("#ff1a1a1e");
                const rust::Vec<uint8_t> probe = term_palette_probe();
                switch_ok &= probe.size() >= 48 && probe[0] == 0xFF && probe[1] == 0xFF
                             && probe[2] == 0xFF;
            }
            // 像素级回归（2026-10-26）：上面几条只证明"配色数据对"，证明不了"画出来对"。
            // `tspans::paint` 的同色缓存曾把哨兵写成 0xFFFFFFFF（正好是纯白），Light 主题
            // 的首条记录命中哨兵、用默认构造的黑色 QColor 铺底 —— 整块终端正文变黑。
            // 这里抓窗格右下角一块空白区，按主题核对亮度。
            if (auto *pane = w.pane_view(0, 0); pane != nullptr && pane->isVisible()) {
                const QImage shot = pane->grab().toImage();
                if (!shot.isNull() && shot.width() > 40 && shot.height() > 40) {
                    const int x0 = shot.width() * 3 / 4, y0 = shot.height() * 3 / 4;
                    double sum = 0.0;
                    int samples = 0;
                    for (int y = y0; y < qMin(y0 + 12, shot.height()); ++y) {
                        for (int x = x0; x < qMin(x0 + 12, shot.width()); ++x) {
                            const QColor c = shot.pixelColor(x, y);
                            sum += (c.red() * 299 + c.green() * 587 + c.blue() * 114) / 1000.0;
                            ++samples;
                        }
                    }
                    const double mean = samples > 0 ? sum / samples : -1.0;
                    const bool want_light = std::string_view(t.id) == "light";
                    const bool pane_ok = want_light ? mean > 170.0 : mean < 90.0;
                    if (!pane_ok) {
                        std::printf("[cpp] C7-c 窗格底色 %.0f 与主题 %s 不符\n", mean, t.id);
                    }
                    switch_ok &= pane_ok;
                }
            }
        }
        prefs_set(snapshot); // 恢复用户配置（含主题 id 与全部字段）
        w.apply_theme();
    }
    std::printf("[cpp] C7-c 主题切换即时生效: %s\n", switch_ok ? "通过" : "失败");
    ok &= switch_ok;

    // 4) 离屏渲染落盘。
    const std::string png_path(png_out.data(), png_out.size());
    if (!png_path.empty()) {
        const QPixmap shot = w.grab();
        const bool saved = shot.save(QString::fromStdString(png_path));
        std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                    saved ? "成功" : "失败", shot.width(), shot.height());
        ok = ok && saved;
    }

    ok &= preferences_dialog_check(&w);
    ok &= terminal_actions_check(&w);
    MainWindow survivor;
    survivor.show();
    ok &= selftest_ui::verify_window_close(w, QString::fromStdString(png_path));
    ok &= selftest_ui::verify_close_preference(survivor);
    const int verdict = ok ? 0 : 1;
    std::printf("[cpp] Phase C ⑦ 自检: %s\n", verdict == 0 ? "通过" : "失败");
    app.exit(verdict);
    return verdict;
}
