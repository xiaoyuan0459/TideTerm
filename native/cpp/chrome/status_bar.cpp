#include "cpp/chrome/selftest_settings.h"
// status_bar 实现（Phase C ⑧ → 2026-09-25 WT 对齐批次 2：**两行**）。
//
// WT 实测口径（docs/reports/qt-windterm-ui-align-2026-09-25.md §2.1）：
//   高 55.7（物理 69）→ 54 = 2×27（kStatusRowHeight）；行 2 右对齐字段集
//   「状态点 · 远端模式 · 窗口 W×H · 行 N · 字符 N · UTF-8 · 时间」，行 1 左「就绪」。
//
// 宿主仍是 QStatusBar（`win->statusBar()` 访问器与既有 objectName 断言不变）：两行内容
// 装进**一个 permanent widget**。代价：QStatusBar 自绘的临时消息区被它盖住 ⇒
// `MainWindow::note()` 改走 `chrome::status_note()` 写行 1 左段（4s 回落「就绪」）。
//
// 字段来源（全部走既有桥，不新增 Rust 字段）：
//   窗口 W×H = term_stats.cols×rows；行 N = cursor_row+1；
//   字符 N = 当前选择文本长度（无选择 = 0，口径见 deliverable §5.2-3）；
//   UTF-8 = 终端编码（与 session_banner 同一口径的固定串，非新事实）。

#include "cpp/i18n.h"
#include "cpp/chrome/status_bar.h"
#include "cpp/chrome/system_badge.h"
#include "cpp/chrome/app_font.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/tab_buttons.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/chrome/elided_label.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

#include <QApplication>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QStatusBar>
#include <QTime>
#include <QDateTime>
#include <QElapsedTimer>
#include <limits>
#include <QTimer>
#include <QVBoxLayout>

namespace chrome {

namespace {

QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }

class FpsLabel final : public ElidedLabel {
public:
    using ElidedLabel::ElidedLabel;
    void sample(qulonglong painted) {
        if (!clock_.isValid()) {
            clock_.start();
            previous_ = painted;
            setText(QStringLiteral("0 fps"));
            return;
        }
        const qint64 elapsed = clock_.elapsed();
        if (elapsed < 1000) return;
        const double delta = painted >= previous_ ? double(painted - previous_) : 0;
        const int rate = int(qMin(delta * 1000 / double(elapsed),
            double(std::numeric_limits<int>::max())));
        previous_ = painted;
        clock_.restart();
        setText(QStringLiteral("%1 fps").arg(rate));
    }
private:
    QElapsedTimer clock_;
    qulonglong previous_ = 0;
};

/// 无消息时的行 1 左段文案。
QString idle_text() { return i18n::tr("就绪"); }

/// 引擎渲染后端名 → 参考「◈ GPU」标签（"gpu …" / "software …"）。
QString backend_text(const QString &raw) {
    return raw.startsWith(QLatin1String("gpu")) ? QStringLiteral("GPU") : i18n::tr("软件");
}

QLabel *make_seg(QWidget *parent, const char *name) {
    QLabel *l = nullptr;
    if (qstrcmp(name, "statusDot") == 0) l = new QLabel(parent);
    else if (qstrcmp(name, "statusFps") == 0) l = new FpsLabel(parent);
    else l = new ElidedLabel(parent);
    l->setObjectName(QLatin1String(name));
    if (qstrcmp(name, "statusMsg") != 0 && qstrcmp(name, "statusKind") != 0)
        set_connection_font(l);
    return l;
}

/// 分段分隔条（`.status-sep` 1×11）。间距由行布局的 spacing 给（各 6px ⇒ 相邻段合计 13px），
/// 不能写成 widget 自身的 contentsMargins——1px 宽的控件带上 padding 后，QSS 背景
/// 只剩 0 宽，分隔条会整条看不见（2026-09-25 实拍确认）。
QFrame *make_sep(QWidget *parent) {
    auto *f = new QFrame(parent);
    f->setObjectName(QLatin1String("statusSep"));
    f->setFrameShape(QFrame::NoFrame);
    f->setFixedSize(1, metrics::kStatusSepHeight);
    return f;
}

/// 单行容器：固定 27 高、横向 16px 内边距（WT 实测左 17 / 右 21 逻辑 px）。
QWidget *make_row(QWidget *parent, const char *name, QHBoxLayout **out) {
    auto *row = new QWidget(parent);
    row->setObjectName(QLatin1String(name));
    row->setFixedHeight(metrics::kStatusRowHeight);
    row->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    auto *lay = new QHBoxLayout(row);
    lay->setContentsMargins(16, 0, 16, 0);
    lay->setSpacing(6);
    *out = lay;
    return row;
}

} // namespace

void build_status_bar(MainWindow *win) {
    QStatusBar *bar = win->statusBar();
    auto *grid = new QWidget(bar);
    grid->setObjectName(QLatin1String("statusGrid"));
    grid->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    auto *rows = new QVBoxLayout(grid);
    rows->setContentsMargins(0, 0, 0, 0);
    rows->setSpacing(0);

    // ---- 行 1：左「就绪」/ 临时消息 + 右 fps · 后端 · 主题 ----
    QHBoxLayout *top_lay = nullptr;
    QWidget *top = make_row(grid, "statusRowTop", &top_lay);
    QLabel *msg = make_seg(top, "statusMsg");
    msg->setText(idle_text());
    msg->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    top_lay->addWidget(msg, 1);
    top_lay->addWidget(make_seg(top, "statusFps"));
    top_lay->addWidget(make_sep(top));
    top_lay->addWidget(make_seg(top, "statusBackend"));
    top_lay->addWidget(make_sep(top));
    top_lay->addWidget(make_seg(top, "statusTheme"));

    // ---- 行 2（右对齐）：状态点 · 远端模式 · 窗口 W×H · 行 N · 字符 N · UTF-8 · 时间 ----
    QHBoxLayout *bot_lay = nullptr;
    QWidget *bottom = make_row(grid, "statusRowBottom", &bot_lay);
    auto *sync = make_seg(bottom, "statusSync");
    sync->setTextFormat(Qt::PlainText); sync->setMaximumWidth(280);
    auto sync_font = sync->font(); sync_font.setBold(true); sync->setFont(sync_font);
    sync->hide(); bot_lay->addWidget(sync);
    bot_lay->addStretch(1);
    QLabel *dot = make_seg(bottom, "statusDot");
    dot->setFixedSize(metrics::kStatusDotSize, metrics::kStatusDotSize);
    bot_lay->addWidget(dot);
    bot_lay->addWidget(make_seg(bottom, "statusKind"));
    bot_lay->addWidget(make_seg(bottom, "statusMode"));
    bot_lay->addWidget(make_sep(bottom));
    bot_lay->addWidget(make_seg(bottom, "statusSize"));
    bot_lay->addWidget(make_sep(bottom));
    bot_lay->addWidget(make_seg(bottom, "statusRows"));
    bot_lay->addWidget(make_sep(bottom));
    bot_lay->addWidget(make_seg(bottom, "statusChars"));
    bot_lay->addWidget(make_sep(bottom));
    QLabel *charset = make_seg(bottom, "statusCharset");
    charset->setText(QStringLiteral("UTF-8"));
    bot_lay->addWidget(charset);
    bot_lay->addWidget(make_sep(bottom));
    bot_lay->addWidget(make_seg(bottom, "statusRight"));
    bot_lay->insertWidget(bot_lay->count() - 1, make_seg(bottom, "statusShell"));

    rows->addWidget(top);
    rows->addWidget(bottom);
    // QStatusBar 内部布局自带约 4px 边距，会把两行 27 挤掉几像素（实拍行高变 ~24）；
    // 清零后 client 高 = 55 − 1px 顶描边 = 54 = 2×27，正好容纳两行。
    if (bar->layout() != nullptr) {
        bar->layout()->setContentsMargins(0, 0, 0, 0);
    }
    bar->setSizeGripEnabled(false); // WT 状态栏无右下角拖拽把；也避免它压住第 1 行右端
    // permanent：临时消息（showMessage）不会把它藏掉。
    bar->addPermanentWidget(grid, 1);
}

void tick_status_bar(MainWindow *win) {
    sync_tab_buttons(win); // 对齐批次 B：标签角标（活动方块/序号/状态点）
    QStatusBar *bar = win->statusBar();
    auto seg = [bar](const char *name) { return bar->findChild<QLabel *>(QLatin1String(name)); };
    QLabel *dot = seg("statusDot");
    QLabel *kind = seg("statusKind");
    QLabel *size = seg("statusSize");
    QLabel *rowpos = seg("statusRows");
    QLabel *chars = seg("statusChars");
    QLabel *backend = seg("statusBackend");
    QLabel *fps = seg("statusFps");
    QLabel *theme = seg("statusTheme");
    QLabel *clock = seg("statusRight");
    if (dot == nullptr || kind == nullptr || size == nullptr || rowpos == nullptr ||
        chars == nullptr || backend == nullptr || fps == nullptr || theme == nullptr ||
        clock == nullptr) {
        return;
    }

    // Count real paint events over elapsed time, independently of chrome refresh frequency.
    unsigned long long painted = 0;
    for (PtyView *v : MainWindow::views(win)) {
        painted += v->painted();
        update_system_badge(v);
    }
    static_cast<FpsLabel *>(fps)->sample(painted);

    // Use the focused view's stable ID, including an auxiliary Shell when no
    // main tabs remain. Zero is a valid ID; UINT32_MAX denotes no live view.
    const auto *active = win->active_view();
    const uint32_t found = active ? active->tab() : UINT32_MAX;
    const bool has = found != UINT32_MAX;
    const uint32_t id = has ? found : 0;
    if (auto *sync_label = seg("statusSync")) {
        const auto info = term_sync_status(win->sync_owner(), found);
        const QString text = info.channel ? i18n::tr("同步：%1（%2）").arg(q(info.name)).arg(info.members) : QString();
        sync_label->setText(text); sync_label->setVisible(!text.isEmpty());
        sync_label->setToolTip(i18n::tr("同通道键盘输入发送给本窗口成员；只读、锁定、离线或正在传输文件的目标不接收"));
    }
    if (auto *mode = seg("statusMode")) {
        mode->setText(!has ? QString() : term_reading(id) ? i18n::tr("本地模式（只读）") : i18n::tr("远程模式（输入）"));
        mode->setToolTip(i18n::tr("Ctrl+Enter 切换输入与阅读模式；i 返回输入"));
    }
    const StatusKind info = status_kind(has ? term_status_kind(id) : 0);
    const QColor state = status_color(QLatin1String(info.name));
    kind->setText(i18n::tr(info.text));
    // 状态色逐段取主题（1s tick 自愈：换主题后下一秒即刷新，无需重建控件）。
    const QString kind_style = QStringLiteral("color: %1;").arg(state.name(QColor::HexArgb));
    const QString dot_style = QStringLiteral("background: %1; border-radius: %2px;")
        .arg(state.name(QColor::HexArgb)).arg(metrics::kStatusDotSize / 2);
    if (kind->styleSheet() != kind_style) kind->setStyleSheet(kind_style);
    if (dot->styleSheet() != dot_style) dot->setStyleSheet(dot_style);
    const TermStats st = has ? term_stats(id) : TermStats{};
    size->setText(i18n::tr("窗口 %1×%2")
                      .arg(has ? int(st.cols) : 0)
                      .arg(has ? int(st.rows) : 0));
    rowpos->setText(i18n::tr("行 %1").arg(has ? int(st.cursor_row) + 1 : 0));
    chars->setText(i18n::tr("字符 %1").arg(has ? int(q(term_selected_text(id)).size()) : 0));
    backend->setText(has ? backend_text(q(term_renderer_name(id))) : QStringLiteral("—"));
    const QString theme_id = q(prefs_get().theme);
    if (theme->property("themeId").toString() != theme_id) {
        for (const auto &item : prefs_themes()) {
            if (q(item.id) != theme_id) continue;
            theme->setText(i18n::tr("主题：%1").arg(q(item.name)));
            theme->setToolTip(theme->text());
            theme->setProperty("themeId", theme_id);
            break;
        }
    }
    if (auto *shell = seg("statusShell")) shell->setText(has ? win->q_title(id) : QString());
    clock->setText(QDateTime::currentDateTime().toString(QStringLiteral("yyyy/M/d HH:mm")));
}

void status_note(QStatusBar *bar, const QString &msg) {
    if (bar == nullptr) {
        return;
    }
    auto *label = bar->findChild<QLabel *>(QLatin1String("statusMsg"));
    if (label == nullptr) {
        return;
    }
    auto *reset = bar->findChild<QTimer *>(QLatin1String("statusNoteTimer"));
    // 空串 = 立即清空回「就绪」（调用方主动复位，也是自检里的复位口）。
    if (msg.isEmpty()) {
        if (reset != nullptr) {
            reset->stop();
        }
        label->setText(idle_text());
        return;
    }
    label->setText(msg);
    // 4s 回落「就绪」。定时器**堆上建 + 挂 bar 为父**（栈帧内声明 QTimer 会随帧析构，
    // 回调引用悬垂即挂死——native/README.md 教训 1）。
    if (reset == nullptr) {
        reset = new QTimer(bar);
        reset->setObjectName(QLatin1String("statusNoteTimer"));
        reset->setSingleShot(true);
        QObject::connect(reset, &QTimer::timeout, bar, [bar] {
            if (auto *l = bar->findChild<QLabel *>(QLatin1String("statusMsg"))) {
                l->setText(idle_text());
            }
        });
    }
    reset->start(4000);
}

} // namespace chrome

// ---- Phase C ⑧ 自检 ----

namespace {
class StyleChanges final : public QObject {
public:
    int count = 0;
protected:
    bool eventFilter(QObject *, QEvent *event) override {
        if (event->type() == QEvent::StyleChange) ++count;
        return false;
    }
};
}

/// 状态栏+日志自检：两行分段齐全且后端取引擎真值 / 日志开关与导出语义。
int qt_boot_c8(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c8";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    bool ok = true;
    MainWindow w;
    w.show();
    chrome::tick_status_bar(&w); // 正常态由 1s tick 驱动，自检里显式跑一次

    // 1) 分段：两行 = 行1「就绪 + fps/后端/主题」、行2「点/模式/窗口/行/字符/编码/时钟」，
    //    分隔条 7 条（行1 两条 + 行2 五条）；后端为引擎真值。
    auto seg = [&w](const char *name) {
        return w.statusBar()->findChild<QLabel *>(QLatin1String(name));
    };
    QLabel *backend = seg("statusBackend");
    QLabel *dot = seg("statusDot");
    QLabel *rowpos = seg("statusRows");
    QLabel *msg = seg("statusMsg");
    const int seps = w.statusBar()->findChildren<QFrame *>(QLatin1String("statusSep")).size();
    const auto *active = w.active_view();
    const TermStats expected = active ? term_stats(active->tab()) : TermStats{};
    const bool bar_ok = active != nullptr && dot != nullptr && seg("statusKind") != nullptr &&
                        seg("statusSize") != nullptr && rowpos != nullptr &&
                        seg("statusChars") != nullptr && seg("statusCharset") != nullptr &&
                        backend != nullptr && seg("statusFps") != nullptr &&
                        seg("statusTheme") != nullptr && msg != nullptr &&
                        seg("statusRight") != nullptr && seps == 7 &&
                        dot->width() == chrome::metrics::kStatusDotSize &&
                        backend->text() == i18n::tr("软件") &&
                        rowpos->text() == i18n::tr("行 %1").arg(int(expected.cursor_row) + 1) &&
                        seg("statusSize")->text() == i18n::tr("窗口 %1×%2")
                            .arg(int(expected.cols)).arg(int(expected.rows));
    std::printf("[cpp] C8-a 状态栏两行分段(页=%d 数=%d 窗口=%s 行=%s 后端=%s): %s\n",
                w.current_page(), w.page_count(),
                seg("statusSize") == nullptr ? "?" : seg("statusSize")->text().toUtf8().constData(),
                rowpos == nullptr ? "?" : rowpos->text().toUtf8().constData(),
                backend == nullptr ? "?" : backend->text().toUtf8().constData(),
                bar_ok ? "通过" : "失败");
    ok &= bar_ok;

    // Repeated metadata refreshes must not repolish unchanged status colors.
    StyleChanges changes;
    seg("statusKind")->installEventFilter(&changes);
    dot->installEventFilter(&changes);
    for (int i = 0; i < 10; ++i) w.refresh_status();
    const bool style_stable = changes.count == 0;
    std::printf("[cpp] C8 unchanged status style events=%d: %s\n", changes.count,
        style_stable ? "PASS" : "FAIL");
    ok &= style_stable;

    // A short burst of real paints is not a new one-second FPS sample.
    auto *fps_label = seg("statusFps");
    const QString fps_before = fps_label->text();
    QElapsedTimer burst;
    burst.start();
    for (int i = 0; i < 10; ++i) {
        w.active_view()->grab();
        w.refresh_status();
    }
    const bool fps_stable = burst.elapsed() < 500 && fps_label->text() == fps_before;
    std::printf("[cpp] C8 subsecond paint burst FPS: %s\n", fps_stable ? "PASS" : "FAIL");
    ok &= fps_stable;

    // 1b) 临时消息走 status_note（两行布局下 QStatusBar 自绘消息区被盖住）：写入即生效；
    //     空串立即回落「就绪」（4s 自动回落由同一个单次定时器负责，这里不复跑等待）。
    chrome::status_note(w.statusBar(), QStringLiteral("测试消息"));
    const bool noted = msg != nullptr && msg->text() == QStringLiteral("测试消息");
    chrome::status_note(w.statusBar(), QString());
    const bool reset_ok = msg != nullptr && msg->text() == i18n::tr("就绪");
    std::printf("[cpp] C8-a2 临时消息出口/回落: %s\n", noted && reset_ok ? "通过" : "失败");
    ok &= noted && reset_ok;

    // 2) 日志开关（桥）：start → active；stop → !active。
    const uint32_t tab = w.pane_ids(0).empty() ? 0 : w.pane_ids(0)[0];
    slog_start(tab);
    const bool on_ok = slog_active(tab);
    slog_stop(tab);
    const bool off_ok = !slog_active(tab);
    std::printf("[cpp] C8-b 日志开关: %s\n", on_ok && off_ok ? "通过" : "失败");
    ok &= on_ok && off_ok;

    // Empty exports cross the bridge as a recoverable error, not a success path.
    bool export_ok = false;
    try { (void)slog_export(tab); }
    catch (const std::exception &e) { export_ok = QString::fromUtf8(e.what()).contains(QStringLiteral("没有日志")); }
    std::printf("[cpp] C8-c 导出空环语义: %s\n", export_ok ? "通过" : "失败");
    ok &= export_ok;

    const std::string png_path(png_out.data(), png_out.size());
    if (!png_path.empty()) {
        app.processEvents(); // 分段文本要经一轮布局才有正确宽度
        const QPixmap shot = w.grab();
        const bool saved = shot.save(QString::fromStdString(png_path));
        std::printf("[cpp] 离屏渲染落盘 %s: %s\n", png_path.c_str(),
                    saved ? "成功" : "失败");
        ok = ok && saved;
    }

    const int verdict = ok ? 0 : 1;
    std::printf("[cpp] Phase C ⑧ 自检: %s\n", verdict == 0 ? "通过" : "失败");
    app.exit(verdict);
    return verdict;
}
