#include "cpp/chrome/selftest_settings.h"
// PtyView 的会话泵编排器（G1 --pty-selftest / G2 --g2-selftest / --pty 交互）。
// Phase C ①：视图带 tab 索引；多标签窗口（MainWindow）在 cpp/main.cpp。

#include "cpp/terminal/terminal_view.h"
#include "cpp/terminal/span_paint_check.h"
#include "cpp/chrome/app_font.h"

#include <QApplication>
#include <QPixmap>
#include <QTimer>

#include <cstdio>
#include <string>

// This is pointer intent owned by this view. Rust still owns the live selection and its bounds.
bool PtyView::begin_select_to(bool column) {
    if (closed_ || !term_selection_active(tab_)) return false;
    cancel_select_to();
    setFocus(Qt::OtherFocusReason);
    selection_target_ = column ? SelectionTarget::Column : SelectionTarget::Stream;
    preedit_.clear();
    setCursor(Qt::CrossCursor);
    setToolTip(i18n::tr("点击选择终点；Esc 取消"));
    update();
    return true;
}

void PtyView::cancel_select_to() {
    if (selection_target_ == SelectionTarget::None) return;
    const bool waiting = selection_target_ != SelectionTarget::Release;
    selection_target_ = SelectionTarget::None;
    selection_target_button_ = Qt::NoButton;
    selection_target_changed_ = false;
    setCursor(Qt::IBeamCursor);
    setToolTip(QString());
    if (waiting) if (auto *win = dynamic_cast<MainWindow *>(window()))
        win->note(i18n::tr("已取消选择到"));
}

bool PtyView::event(QEvent *e) {
    if (selection_target_ == SelectionTarget::None) {
        if (!closed_ && e->type() == QEvent::KeyPress) {
            auto *key = static_cast<QKeyEvent *>(e);
            if ((key->key() == Qt::Key_Tab || key->key() == Qt::Key_Backtab) &&
                !(key->modifiers() & (Qt::ControlModifier | Qt::AltModifier)) &&
                term_reading(tab_) && term_selection_status(tab_).count > 1) {
                keyPressEvent(key);
                if (auto *win = dynamic_cast<MainWindow *>(window())) {
                    const SelectionStatus state = term_selection_status(tab_);
                    win->note(i18n::tr("活动选区 %1 / %2").arg(state.active).arg(state.count));
                    win->refresh_chrome();
                }
                return true;
            }
        }
        return QWidget::event(e);
    }
    switch (e->type()) {
    case QEvent::FocusOut:
    case QEvent::Hide:
    case QEvent::WindowDeactivate:
    case QEvent::Resize:
        cancel_select_to();
        break;
    case QEvent::ShortcutOverride:
        e->accept(); return true;
    case QEvent::KeyPress:
    case QEvent::InputMethod:
    case QEvent::Wheel:
        cancel_select_to();
        e->accept(); return true;
    case QEvent::MouseMove:
        e->accept(); return true; // No application mouse reports while choosing the endpoint.
    case QEvent::MouseButtonPress:
    case QEvent::MouseButtonDblClick: {
        if (selection_target_ == SelectionTarget::Release) { e->accept(); return true; }
        auto *mouse = static_cast<QMouseEvent *>(e);
        const bool column = selection_target_ == SelectionTarget::Column;
        const QPointF point = grid_pt(mouse->position());
        selection_target_button_ = mouse->button();
        selection_target_changed_ = mouse->button() == Qt::LeftButton &&
            term_selection_extend_to(tab_, float(point.x()), float(point.y()), column);
        selection_target_ = SelectionTarget::Release;
        setCursor(Qt::IBeamCursor);
        setToolTip(QString());
        if (!selection_target_changed_) if (auto *win = dynamic_cast<MainWindow *>(window()))
            win->note(mouse->button() == Qt::LeftButton ? i18n::tr("选择已失效，请重新选择")
                                                       : i18n::tr("已取消选择到"));
        update(); e->accept(); return true;
    }
    case QEvent::MouseButtonRelease: {
        auto *mouse = static_cast<QMouseEvent *>(e);
        if (selection_target_ == SelectionTarget::Release && mouse->button() == selection_target_button_) {
            const bool changed = selection_target_changed_;
            cancel_select_to();
            if (changed) {
                term_selection_end(tab_); // Qt host already owns clipboard publication.
                if (auto *win = dynamic_cast<MainWindow *>(window())) {
                    win->note(i18n::tr("选择已更新"));
                    win->refresh_chrome();
                }
            }
            update();
        }
        e->accept(); return true;
    }
    default: break;
    }
    return QWidget::event(e);
}

int qt_boot_pty(rust::Str feed_cmd, rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-pty";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    span_paint_check::prepare_fonts();
    chrome::init_app_font();

    PtyView view(term_new_tab());
    view.show();

    // 会话泵：20ms poll（drain PTY 事件 → 喂引擎），有输出才 requestUpdate。
    QTimer pump;
    QObject::connect(&pump, &QTimer::timeout, [&view] { view.poll_and_update(); });
    pump.start(20);

    // 自检时序：300ms 后喂命令 → 引擎跑输出 → 泵渲染 → 断言 + 落盘。
    const std::string feed(feed_cmd.data(), feed_cmd.size());
    QTimer::singleShot(300, [&view, feed] {
        std::printf("[cpp] 喂入命令: %s\n", feed.c_str());
        std::string cmd = feed;
        cmd += "\r";
        term_write(view.tab(), rust::Slice<const uint8_t>(
            reinterpret_cast<const uint8_t *>(cmd.data()), cmd.size()));
    });

    int verdict = 1;
    QTimer::singleShot(2600, [&] {
        view.repaint();
        const std::string png_path(png_out.data(), png_out.size());
        if (!png_path.empty()) {
            const QPixmap shot = view.grab();
            const bool saved = shot.save(QString::fromStdString(png_path));
            std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                        saved ? "成功" : "失败", shot.width(), shot.height());
        }
        // 吞吐断言先跑：满载帧计时会注入 60 行文本把标记行滚走，之后网格不再是被测对象。
        verdict = term_pty_selftest_assert() ? 0 : 1;
        if (!span_paint_check::verify(QString::fromStdString(png_path))) verdict = 1;
        // 原生字形路径的每帧成本：累计 paintEvent 墙钟 + 整窗单帧（常规 / 满载）。
        const double total_ms = double(view.paint_ns()) / 1e6;
        QElapsedTimer frame;
        frame.start();
        const QPixmap full = view.grab();
        const double normal_ms = double(frame.nsecsElapsed()) / 1e6;
        std::string dense;
        for (int r = 0; r < 60; ++r) {
            for (int c = 0; c < 200; ++c) {
                dense += char('A' + (r + c) % 26);
            }
            dense += "\r\n";
        }
        term_feed_inject(view.tab(), rust::Slice<const uint8_t>(
            reinterpret_cast<const uint8_t *>(dense.data()), dense.size()));
        frame.restart();
        view.grab(); // 强制整屏重绘（不落盘，只计时）
        const double dense_ms = double(frame.nsecsElapsed()) / 1e6;
        std::printf("[cpp] 字形绘制计时: %u 帧 / %.1f ms（均值 %.2f ms/帧），整窗 %dx%d "
                    "常规帧 %.2f ms / 满载帧 %.2f ms\n",
                    view.painted(), total_ms,
                    view.painted() ? total_ms / view.painted() : 0.0,
                    full.width(), full.height(), normal_ms, dense_ms);
        std::printf("[cpp] G1 PTY 自检: %s\n", verdict == 0 ? "通过" : "失败");
        app.exit(verdict);
    });

    return app.exec();
}

int qt_boot_g2(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-g2";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;

    PtyView view(term_new_tab());
    view.show();

    QTimer pump;
    QObject::connect(&pump, &QTimer::timeout, [&view] { view.poll_and_update(); });
    pump.start(20);

    int verdict = 1;
    bool anchor_ok = false;
    bool preedit_ok = false;
    bool submitted = false;
    QElapsedTimer scenario_clock;
    scenario_clock.start();
    QTimer ime_submit, verify;
    QTimer::singleShot(600, [&view, &anchor_ok, &preedit_ok] {
        // 1) 锚定：ImCursorRectangle 必须等于引擎光标单元格 × 度量（surface 坐标）。
        TermStats st = term_stats(view.tab());
        QVariant q = view.ime_query(Qt::ImCursorRectangle);
        const QRectF anchor = q.toRectF();
        const qreal dpr = view.devicePixelRatioF();
        const qreal pad_x = std::round(chrome::metrics::kPanePadX * dpr) / dpr;
        const qreal pad_y = std::round(chrome::metrics::kPanePadY * dpr) / dpr;
        auto near = [dpr](qreal a, qreal b) { return qAbs(a - b) <= 0.01 / dpr; };
        // 正文从行号栏右边开始：引擎的 caret 列 + 栏宽才是屏上的格位（`term_caret_rect`
        // 已含栏宽，所以期望值也必须加，否则开栏时按无栏算会差整整一栏）。
        const qreal gutter_px = st.gutter_cells * st.cell_w / dpr;
        anchor_ok = !anchor.isNull()
            && near(anchor.x(), pad_x + gutter_px + st.cursor_col * st.cell_w / dpr)
            && near(anchor.y(), pad_y + st.cursor_row * st.cell_h / dpr)
            && near(anchor.width(), st.cell_w / dpr)
            && near(anchor.height(), st.cell_h / dpr);
        std::printf("[cpp] G2-a 组合窗锚定: %s (x=%.0f y=%.0f w=%.0f h=%.0f，期望 cell(%u,%u)+栏%u×%ux%u)\n",
                    anchor_ok ? "通过" : "失败", anchor.x(), anchor.y(),
                    anchor.width(), anchor.height(), st.cursor_col, st.cursor_row,
                    st.gutter_cells, st.cell_w, st.cell_h);
        // 2) 预编辑：合成预编辑事件 → 上屏（不进 PTY）。
        view.inject_ime(QString(), QStringLiteral("中文测"));
        view.repaint();
        preedit_ok = view.preedit() == QStringLiteral("中文测");
        std::printf("[cpp] G2-b 预编辑注入: %s（preedit=\"%s\"，paint 已含下划线绘制）\n",
                    preedit_ok ? "通过" : "失败",
                    view.preedit().toUtf8().constData());
    });

    QObject::connect(&ime_submit, &QTimer::timeout, &view, [&] {
        const auto output = term_screen_text(view.tab());
        const QString text = QString::fromUtf8(output.data(), int(output.size())).trimmed();
#ifdef Q_OS_WIN
        const bool ready = text.contains(QStringLiteral("PS ")) && text.endsWith(QLatin1Char('>'));
#else
        const bool ready = text.endsWith(QLatin1Char('$')) || text.endsWith(QLatin1Char('#')) ||
            text.endsWith(QLatin1Char('%')) || text.endsWith(QLatin1Char('>'));
#endif
        if (!ready) return;
        // 3) 提交串：合成提交事件 → PTY → PowerShell 回显 → 引擎渲染。
        view.inject_ime(QStringLiteral("中文测试123"), QString());
        submitted = true;
        ime_submit.stop();
    });
    QTimer::singleShot(900, &view, [&] { ime_submit.start(20); });

    QObject::connect(&verify, &QTimer::timeout, &view, [&] {
        const auto output = term_screen_text(view.tab());
        const bool echoed_now = QString::fromUtf8(output.data(), int(output.size())).contains(QStringLiteral("中文测试123"));
        if ((!submitted || !echoed_now) && scenario_clock.elapsed() < 10000) return;
        verify.stop();
        ime_submit.stop();
        view.repaint();
        const std::string png_path(png_out.data(), png_out.size());
        if (!png_path.empty()) {
            const QPixmap shot = view.grab();
            const bool saved = shot.save(QString::fromStdString(png_path));
            std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                        saved ? "成功" : "失败", shot.width(), shot.height());
        }
        const bool echoed = term_g2_selftest_assert();
        verdict = anchor_ok && preedit_ok && echoed ? 0 : 1;
        std::printf("[cpp] G2 IME 自检: %s\n", verdict == 0 ? "通过" : "失败");
        app.exit(verdict);
    });
    verify.start(20);

    return app.exec();
}

int qt_run_pty() {
    static int argc = 1;
    static char arg0[] = "native-pty";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    PtyView view(term_new_tab());
    view.show();
    QTimer pump;
    QObject::connect(&pump, &QTimer::timeout, [&view] { view.poll_and_update(); });
    pump.start(20);
    return app.exec();
}
