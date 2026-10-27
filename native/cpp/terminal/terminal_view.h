// PtyView —— 自绘终端视图（QWidget + paintEvent；ADR-012 铁律 3：不用 QTextEdit 系）。
//
// Phase C ①：视图绑定**标签索引**（Rust 侧注册表；C++ 不持业务状态——铁律 2）。
// 像素来源：term_spans(tab) 返回**无像素**绘制记录流（见 cpp/terminal/span_paint.h），
// 字形由 Qt 的 DirectWrite 文本栈光栅化 —— Rust 侧不再为每个窗格保有整屏 RGBA 双缓冲。
// 记录每帧现取、绝不跨 paint 缓存。
//
// G2 IME：WA_InputMethodEnabled + inputMethodQuery(ImCursorRectangle) 锚定组合窗
// 到引擎光标（term_caret_rect = 光标单元格 × 字体度量）；inputMethodEvent 把
// 提交串送 PTY（term_ime_commit，与键盘同路）、预编辑串上屏（光标处下划线绘制，
// 不进 PTY —— 与终端惯例一致）。
#pragma once
#include "cpp/chrome/text_marks.h"

#include <QCoreApplication>
#include <QApplication>
#include <QClipboard>
#include <QElapsedTimer>
#include <QDesktopServices>
#include <QUrl>
#include <QKeyEvent>
#include <QInputMethod>
#include <QLabel>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QShowEvent>
#include <QWheelEvent>
#include <QWidget>

#include <string>
#include <utility>
#include "cpp/i18n.h"
#include "cpp/main_window.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/term_menu.h"
#include "cpp/chrome/menu_style.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/terminal/span_paint.h"
#include "cpp/terminal/selection_press.h"
#include "cpp/terminal/terminal_history.h"
#include "cpp/terminal/terminal_completion.h"

/// 主题色 + 固定 alpha（终端覆盖层：光标行/查找/选区）。
inline QColor tint(QColor c, int alpha) {
    c.setAlpha(alpha);
    return c;
}

// ---- 中立键码（与 rust/term.rs 的 key_from_code 对齐；v1 §6.1 R5 桥面约定） ----
inline constexpr uint32_t kKeyChar = 0;
inline constexpr uint32_t kKeyUp = 1, kKeyDown = 2, kKeyLeft = 3, kKeyRight = 4;
inline constexpr uint32_t kKeyHome = 5, kKeyEnd = 6, kKeyPageUp = 7, kKeyPageDown = 8;
inline constexpr uint32_t kKeyInsert = 9, kKeyDelete = 10, kKeyBackspace = 11;
inline constexpr uint32_t kKeyTab = 12, kKeyBacktab = 13, kKeyReturn = 14, kKeyEscape = 15;
inline constexpr uint32_t kKeyF1 = 32; // F(n) = kKeyF1 + n - 1（F1..F12 → 32..43）
inline constexpr uint32_t kModShift = 1, kModAlt = 2, kModCtrl = 4;

inline uint32_t mods_of(const QKeyEvent *e) {
    uint32_t m = 0;
    if (e->modifiers() & Qt::ShiftModifier) m |= kModShift;
    if (e->modifiers() & Qt::AltModifier) m |= kModAlt;
    if (e->modifiers() & Qt::ControlModifier) m |= kModCtrl;
    if (e->modifiers() & Qt::KeypadModifier) m |= 8;
    return m;
}

inline uint32_t neutral_code(const QKeyEvent *e) {
    switch (e->key()) {
        case Qt::Key_Up: return kKeyUp;
        case Qt::Key_Down: return kKeyDown;
        case Qt::Key_Left: return kKeyLeft;
        case Qt::Key_Right: return kKeyRight;
        case Qt::Key_Home: return kKeyHome;
        case Qt::Key_End: return kKeyEnd;
        case Qt::Key_PageUp: return kKeyPageUp;
        case Qt::Key_PageDown: return kKeyPageDown;
        case Qt::Key_Insert: return kKeyInsert;
        case Qt::Key_Delete: return kKeyDelete;
        case Qt::Key_Backspace: return kKeyBackspace;
        case Qt::Key_Tab: return kKeyTab;
        case Qt::Key_Backtab: return kKeyBacktab;
        case Qt::Key_Return:
        case Qt::Key_Enter: return kKeyReturn;
        case Qt::Key_Escape: return kKeyEscape;
        default:
            if (e->key() >= Qt::Key_F1 && e->key() <= Qt::Key_F12) {
                return kKeyF1 + (e->key() - Qt::Key_F1);
            }
            return kKeyChar; // 可打印字符走 text（Rust 侧 Key::classify）
    }
}

class PtyView : public QWidget {
public:
    /// 绑定标签索引（Rust 侧 term_new_tab 创建会话）。
    explicit PtyView(uint32_t tab, bool timestamps = false) : tab_(tab), timestamps_(timestamps) {
        setObjectName(QStringLiteral("ptyView-%1").arg(tab));
        resize(800, 500);
        setFocusPolicy(Qt::StrongFocus);
        setMouseTracking(true);
        setAttribute(Qt::WA_InputMethodEnabled, true); // G2：启用输入法管线
        rebuild_font_variants();
        connect(qGuiApp, &QGuiApplication::fontDatabaseChanged, this, [this] { glyph_layouts_.clear(); });
        history_ = new chrome::TerminalHistory(this);
        suggestions_ = new TerminalSuggestions(this);
        // Wait for show/layout: QWidget's constructor geometry is only 100x30,
        // which would briefly shrink a new ConPTY to one column/row.
    }

    uint32_t tab() const { return tab_; }
    uint32_t painted() const { return painted_; }
    QString preedit() const { return preedit_; }
    /// 累计 paintEvent 墙钟耗时（纳秒）——原生字形路径的每帧成本取证口径。
    qint64 paint_ns() const { return paint_ns_; }

    /// 会话已在 Rust 侧关闭（term_close_tab 前调用）。此后本视图不再发起任何
    /// term_* 桥调用：deleteLater 排队期间仍可能收到绘制/输入/IME 事件，而
    /// with_tab 对已删 id 会 panic 跨 FFI abort（0xC0000409，同类历史见 R-10）。
    void mark_closed() { cancel_select_to(); history_->cancel(); suggestions_->cancel(); glyph_layouts_.clear(); closed_ = true; preedit_.clear(); setAttribute(Qt::WA_InputMethodEnabled, false); }
    bool is_closed() const { return closed_; }
    bool begin_select_to(bool column);

    void sync_reading_mode() {
        if (closed_) return;
        const bool enabled = !term_reading(tab_);
        if (testAttribute(Qt::WA_InputMethodEnabled) == enabled) return;
        setAttribute(Qt::WA_InputMethodEnabled, enabled);
        preedit_.clear();
        if (hasFocus()) {
            if (!enabled) QApplication::inputMethod()->reset();
            QApplication::inputMethod()->update(Qt::ImEnabled | Qt::ImCursorRectangle);
        }
        if (auto *win = dynamic_cast<MainWindow *>(window())) win->refresh_chrome();
        update();
    }

    bool poll_and_update(bool pump_backend = true) {
        if (closed_) return false;
        sync_reading_mode();
        if (context_take_bell(tab_)) QApplication::beep();
        term_cursor_tick(tab_, hasFocus() && isVisible());
        const bool changed = pump_backend ? term_poll(tab_) : term_take_redraw(tab_);
        if (changed) {
            update();
        }
        history_->sync();
        suggestions_->sync(changed);
        return changed;
    }

    /// G2 自检入口：把合成 QInputMethodEvent 注入本视图（真实 Qt IME 分发路径，
    /// 只是候选来自代码而非系统输入法）。Qt6：预编辑走构造器（无独立 setter）。
    void inject_ime(const QString &commit, const QString &preedit) {
        QInputMethodEvent ev(preedit, QList<QInputMethodEvent::Attribute>{});
        if (!commit.isEmpty()) {
            ev.setCommitString(commit);
        }
        QCoreApplication::sendEvent(this, &ev);
    }

    /// 公共包装：inputMethodQuery 是 protected，自检从外部读取锚定矩形。
    QVariant ime_query(Qt::InputMethodQuery query) {
        return inputMethodQuery(query);
    }

protected:
    bool event(QEvent *e) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery query) const override {
        switch (query) {
            case Qt::ImEnabled:
                return !closed_ && !term_reading(tab_);
            case Qt::ImHints:
                return int(Qt::ImhNone);
            case Qt::ImCursorRectangle: {
                if (closed_) return QRectF();
                Rect r = term_caret_rect(tab_); // 引擎光标单元格 × 字体度量
                const qreal dpr = devicePixelRatioF();
                const qreal x = std::round((chrome::metrics::kPanePadX + timestamp_gutter_width()) * dpr) / dpr;
                const qreal y = std::round(chrome::metrics::kPanePadY * dpr) / dpr;
                return QRectF(r.x + x, r.y + y, r.w, r.h);
            }
            default:
                return QWidget::inputMethodQuery(query);
        }
    }

    void inputMethodEvent(QInputMethodEvent *e) override {
        if (closed_ || term_reading(tab_)) {
            preedit_.clear(); update(); e->accept(); return;
        }
        // 预编辑：上屏显示（光标处、下划线），不进 PTY。
        preedit_ = e->preeditString();
        // 提交串：与键盘同路写 PTY（PowerShell/PSReadLine 回显后由引擎渲染）。
        if (!closed_ && !e->commitString().isEmpty()) {
            const QByteArray utf8 = e->commitString().toUtf8();
            term_ime_commit(tab_, rust::Str(utf8.constData(), utf8.size()));
        }
        update();
        e->accept();
    }

    void paintEvent(QPaintEvent *) override {
        const QColor bg = chrome::ui_colors().term_bg;
        if (closed_) {
            // 会话已终止：只铺底色，不再查询引擎（spans/caret 都是对端状态）。
            QPainter painter(this);
            painter.fillRect(rect(), bg);
            ++painted_;
            return;
        }
        if (window()->property("memoryProbeBlankPty").toBool()) {
            QPainter painter(this);
            painter.fillRect(rect(), bg);
            ++painted_;
            return;
        }
        // 记录是物理像素；tspans::paint 除以 dpr 落到本控件的逻辑坐标系（下方 translate
        // 只负责窗格 padding，缩放仍由 painter 的 devicePixelRatio 承担）。
        rebuild_font_variants();
        QElapsedTimer clock;
        clock.start();
        QPainter painter(this);
        painter.setRenderHint(QPainter::TextAntialiasing);
        painter.fillRect(rect(), bg);
        // 网格区整体内缩 kPanePadX/kPanePadY（参考窗格 padding），引擎给的矩形/光标
        // 坐标都是**网格坐标** ⇒ 统一 translate，绘制侧不逐处加减。
        //
        // 平移必须吸附到**物理像素网格**：Qt 在这台机器上用 ClearType 亚像素渲染，引擎格位
        // 是整数物理像素（cell_w = adv.round()），但 12 逻辑 px 的 padding 乘 dpr(≈1.24) 是
        // 14.88 物理 px —— 非整平移让每个字形落在不同亚像素相位上，边缘出现彩色条纹（实拍
        // 灰字样本出现 #72A4BB/#8B571C 级别的彩边）。吸附后整幅文字回到整数物理格位，
        // 彩色边缘显著收敛（与 WindTerm 同样的整格位观感）。
        const qreal dpr = devicePixelRatioF();
        auto snap_px = [dpr](qreal v) { return std::round(v * dpr) / dpr; };
        painter.translate(snap_px(chrome::metrics::kPanePadX + timestamp_gutter_width()),
                          snap_px(chrome::metrics::kPanePadY));
        painter.setClipRect(QRectF(0, 0, grid_w(),
                                   height() - 2 * chrome::metrics::kPanePadY));
        QRectF caret_line;
        if (window()->property("highlightCaretLine").toBool()) {
            const Rect caret = term_caret_rect(tab_);
            caret_line = QRectF(0, caret.y, width(), caret.h);
        }
        tspans::paint(painter, term_spans(tab_), devicePixelRatioF(), fonts_, glyph_layouts_, caret_line,
                      tint(chrome::ui_colors().term_fg, 22));
        chrome::paint_marks(painter, tab_);
        for (const Rect &r : term_search_rects(tab_)) {
            painter.fillRect(QRectF(r.x, r.y, r.w, r.h), tint(chrome::ui_colors().warn, 75));
        }
        const Rect active_match = term_search_active_rect(tab_);
        if (active_match.w > 0 && active_match.h > 0) {
            painter.fillRect(QRectF(active_match.x, active_match.y,
                                    active_match.w, active_match.h),
                             tint(chrome::ui_colors().warn, 125));
        }
        for (const Rect &r : term_selection_rects(tab_)) {
            painter.fillRect(QRectF(r.x, r.y, r.w, r.h), tint(chrome::ui_colors().accent, 90));
        }
        if (term_selection_status(tab_).count > 1) {
            for (const SelectionRect &r : term_selection_active_rects(tab_))
                painter.fillRect(QRectF(r.x, r.y, r.w, r.h), tint(chrome::ui_colors().accent, 70));
        }
        // G2：预编辑串绘制在组合窗锚位（光标矩形右上角延伸），下划线标记。
        if (!preedit_.isEmpty()) {
            Rect r = term_caret_rect(tab_);
            painter.setFont(fonts_[0]); // 与网格同一张脸（此前硬编码 Consolas 14 px）
            painter.setPen(QColor(0xd0, 0xca, 0xf5));
            const QPointF anchor(qreal(r.x), qreal(r.y + r.h));
            painter.drawText(anchor, preedit_);
            const QFontMetrics fm(fonts_[0]);
            const int w = fm.horizontalAdvance(preedit_);
            painter.drawLine(QPointF(anchor.x(), anchor.y() + 2),
                             QPointF(anchor.x() + w, anchor.y() + 2));
        }
        for (const auto &marker : term_fold_markers(tab_)) {
            painter.save();
            // Leave one physical pixel on either side of the gutter cell.
            const qreal side = qMin(qreal(marker.w) - 2.0 / dpr, qreal(9));
            const qreal arm = qMin(qreal(2), side / 3);
            const QRectF box(marker.x + (marker.w - side) / 2, marker.y + (marker.h - side) / 2, side, side);
            painter.setPen(chrome::ui_colors().text_secondary); painter.setBrush(chrome::ui_colors().term_bg);
            painter.drawRect(box.adjusted(0.5, 0.5, -0.5, -0.5));
            painter.drawLine(QPointF(box.left() + arm, box.center().y()), QPointF(box.right() - arm, box.center().y()));
            if (marker.folded) painter.drawLine(QPointF(box.center().x(), box.top() + arm), QPointF(box.center().x(), box.bottom() - arm));
            painter.restore();
        }
        painter.resetTransform();
        // 裁剪是在设备坐标系定型的（translate 时已换算）⇒ 复位变换后必须解掉，
        // 否则下面贴边的描边/状态文字整条落在裁剪区外，画了等于没画。
        painter.setClipping(false);
        if (timestamp_gutter_width() > 0) {
            painter.setFont(fonts_[0]);
            painter.setPen(chrome::ui_colors().text_secondary);
            // 时间列同样吸附物理像素（与网格同一原点规则），否则灰字彩边最明显。
            for (const auto &line : context_line_times(tab_)) {
                painter.drawText(QPointF(snap_px(chrome::metrics::kPanePadX),
                    snap_px(chrome::metrics::kPanePadY + line.baseline)),
                    QString::fromUtf8(line.stamp.data(), int(line.stamp.size())));
            }
        }
        // 活动窗格描边（1px accent）已于 2026-10-01 按用户要求移除：终端外框不画任何
        // 描边（WindTerm 同款观感）；焦点由标签状态方块与状态栏「运行中」表达。
        paint_ns_ += clock.nsecsElapsed();
        ++painted_;
    }

    void resizeEvent(QResizeEvent *) override {
        rebuild_font_variants();
        if (!closed_ && isVisible() && grid_w() > 1 && grid_h() > 1)
            term_resize_px(tab_, static_cast<float>(grid_w()), static_cast<float>(grid_h()));
    }

    void showEvent(QShowEvent *e) override {
        // 落到具体屏幕后校正 dpr（多屏不同缩放）；term_set_scale 同值即无操作，
        // 仅在 dpr 真变化时重置字体/网格，故重复 show 无副作用。
        term_set_scale(static_cast<float>(devicePixelRatioF()));
        rebuild_font_variants();
        if (!closed_ && grid_w() > 1 && grid_h() > 1)
            term_resize_px(tab_, static_cast<float>(grid_w()), static_cast<float>(grid_h()));
        QWidget::showEvent(e);
    }

public:
    /// 像素 → 网格坐标（鼠标命中测试要减掉窗格 padding）。
    QPointF grid_pt(const QPointF &p) const {
        return QPointF(p.x() - chrome::metrics::kPanePadX - timestamp_gutter_width(), p.y() - chrome::metrics::kPanePadY);
    }
    int grid_w() const { return qMax(1, width() - 2 * chrome::metrics::kPanePadX - timestamp_gutter_width() - chrome::TerminalHistory::kWidth); }
    int grid_h() const { return qMax(1, height() - 2 * chrome::metrics::kPanePadY); }

protected:
    void keyPressEvent(QKeyEvent *e) override {
        // Shift+PgUp/PgDn/Home/End 的回滚判定在 Rust（term_key 内，用引擎自己的
        // scroll_page_key/scroll_edge_key）——这里只负责按键转发后重绘。
        if (!closed_) {
            if (e->key() == Qt::Key_Escape && e->modifiers() == Qt::NoModifier &&
                term_selection_status(tab_).count > 1) {
                term_deselect(tab_);
                if (auto *win = dynamic_cast<MainWindow *>(window())) win->refresh_chrome();
                update(); e->accept(); return;
            }
            if (e->isAutoRepeat() && neutral_code(e) == kKeyReturn && (mods_of(e) & 7) == kModCtrl) {
                e->accept(); return;
            }
            if ((e->text() == QStringLiteral("/") || e->text() == QStringLiteral("?")) &&
                (mods_of(e) & (kModCtrl | kModAlt)) == 0 && term_reading_search_ready(tab_)) {
                if (auto *win = dynamic_cast<MainWindow *>(window())) {
                    if (auto *find = chrome::find_menu_action(win->menuBar(), QStringLiteral("search:find"))) find->trigger();
                    e->accept(); return;
                }
            }
            term_key(tab_, neutral_code(e), mods_of(e), e->text().toUtf8().constData());
            sync_reading_mode();
        }
        update();
        e->accept();
    }

    void wheelEvent(QWheelEvent *e) override {
        const double dy = e->angleDelta().y() / 120.0;
        if (!closed_) {
            term_wheel(tab_, static_cast<int>(dy * 3.0),
                       static_cast<float>(grid_pt(e->position()).x()),
                       static_cast<float>(grid_pt(e->position()).y()));
        }
        update();
    }

    void mousePressEvent(QMouseEvent *e) override;
    void mouseDoubleClickEvent(QMouseEvent *e) override;

    void mouseMoveEvent(QMouseEvent *e) override;

    void mouseReleaseEvent(QMouseEvent *e) override;

private:
    // Keep room for ten text cells before showing the optional timestamp column.
    // A fixed gutter can consume a narrow split's entire viewport and suppress
    // resize, leaving both the grid and PTY at their previous dimensions.
    int timestamp_gutter_width() const {
        return width() >= 2 * chrome::metrics::kPanePadX + timestamp_width_ + min_text_width_ + chrome::TerminalHistory::kWidth
            ? timestamp_width_ : 0;
    }

    /// 终端字形字体的五个变体（0..3 = bold<<1 | italic，4 = 中日韩单族脸，与
    /// tspans::Variants 同序）。
    /// 只在字号或 dpr 真变化时重建 —— 每帧构造 QFont 会吃掉整帧预算。
    /// 族名与 Rust 侧度量来源同一个 `term_font_family()`，否则格位与字形会各选各的脸。
    void rebuild_font_variants() {
        const uint32_t px = term_font_px();
        const qreal dpr = devicePixelRatioF();
        if (px == font_px_ && qFuzzyCompare(dpr, font_dpr_)) {
            return;
        }
        const int previous_gutter = timestamp_gutter_width();
        font_px_ = px;
        font_dpr_ = dpr;
        const rust::String family = term_font_family();
        QFont base(QString::fromUtf8(family.data(), int(family.size())));
        base.setPixelSize(int(px)); // 逻辑像素 = 引擎 FONT_SIZE；painter 再按 dpr 放大
        base.setStyleStrategy(QFont::StyleStrategy(QFont::PreferAntialias | QFont::NoSubpixelAntialias));
        base.setHintingPreference(QFont::PreferFullHinting);
        for (int i = 0; i < 4; ++i) {
            fonts_[i] = base;
            fonts_[i].setBold(i & 2);
            fonts_[i].setItalic(i & 1);
        }
        // 中日韩那一格换一张**自己覆盖中文**的单族脸。等宽脸没有汉字，交给 Qt 自动回退会
        // 一次性建出整套回退引擎链（实测窗格 9.2 → 53 MB）；自己按码位切换，两侧都只有
        // 一个族名，`QFontEngineMulti` 根本不会出现。族名跟应用字体同源（生产 = YaHei UI）。
        QFont cjk(QApplication::font().family());
        cjk.setPixelSize(int(px));
        cjk.setStyleStrategy(QFont::StyleStrategy(QFont::PreferAntialias | QFont::NoSubpixelAntialias));
        cjk.setHintingPreference(QFont::PreferFullHinting);
        fonts_[tspans::kCjkSlot] = cjk;
        timestamp_width_ = timestamps_ ? QFontMetrics(base).horizontalAdvance(QStringLiteral("[00:00:00] ")) + 4 : 0;
        min_text_width_ = QFontMetrics(base).horizontalAdvance(QStringLiteral("0000000000"));
        // Font/DPI changes can reveal or hide the gutter without a QWidget resize.
        if (!closed_ && isVisible() && previous_gutter != timestamp_gutter_width() &&
            grid_w() > 1 && grid_h() > 1)
            term_resize_px(tab_, static_cast<float>(grid_w()), static_cast<float>(grid_h()));
    }

    uint32_t tab_;      // 标签索引（Rust 注册表；C++ 不持业务状态）
    bool timestamps_ = false;
    bool closed_ = false; // 会话已终止：禁止再发 term_* 桥调用（见 mark_closed）
    int timestamp_width_ = 0;
    int min_text_width_ = 0;
    uint32_t painted_ = 0;
    QString preedit_;   // G2：预编辑串（光标处下划线显示，不进 PTY）
    bool selecting_ = false;
    chrome::TerminalHistory *history_ = nullptr;
    TerminalSuggestions *suggestions_ = nullptr;
    bool fold_press_ = false;
    enum class SelectionTarget { None, Stream, Column, Release };
    SelectionTarget selection_target_ = SelectionTarget::None;
    Qt::MouseButton selection_target_button_ = Qt::NoButton;
    bool selection_target_changed_ = false;
    void cancel_select_to();
    QString pressed_link_;
    QPointF link_origin_;
    tspans::Variants fonts_;
    tspans::AsciiLayouts glyph_layouts_;
    uint32_t font_px_ = 0;
    qreal font_dpr_ = 0.0;
    qint64 paint_ns_ = 0;
};
