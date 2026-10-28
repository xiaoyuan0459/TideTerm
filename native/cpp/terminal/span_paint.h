// 无像素绘制记录流 → 屏幕（原生字形绘制路径）。
//
// 记录由 Rust 侧 `tiderm_terminal::spans` 逐帧采集，经 `term_spans(idx)` 过桥：
// 每 6 个 u32 一条，坐标是**物理像素**，颜色按内存字节序 `R | G<<8 | B<<16 | A<<24`
// 打包（与帧缓冲的 RGBA8 同序，不是 QRgb 的 0xAARRGGBB —— 混用会 R/B 互换）。
// 这里把它翻译成逻辑像素下的 fillRect / drawText：字形改由 Qt 的 DirectWrite 文本栈
// 光栅化，Rust 侧不再持有每窗格两份全屏 RGBA 缓冲。
//
// 逐格 drawText（而非整段 run）是刻意的：run 绘制会让 Qt 按字体的自然 advance 排字，
// 而格位 x 由引擎取整后的整数 cell_w 决定，两者一旦不等（Cascadia 14 px 的 advance 是
// 8.4）每个字累积 0.4 px 漂移，80 列就偏出 32 px。逐格定位把「格位」与「字形」解耦。
#pragma once

#include <QColor>
#include <QFont>
#include <QPainter>
#include <QPointF>
#include <QRectF>
#include <QString>
#include <QTextLayout>

#include <array>
#include <cstdint>
#include <memory>

#include "cpp/bridge_shim.h"

namespace tspans {

inline constexpr int kStride = 6;
inline constexpr quint32 kKindRect = 0;
inline constexpr quint32 kKindText = 1;
inline constexpr quint32 kFlagBold = 1;
inline constexpr quint32 kFlagItalic = 2;

/// 5 个字面：0..3 = (bold ? 2 : 0) | (italic ? 1 : 0)，4 = 中日韩专用单族字面。
using Variants = std::array<QFont, 5>;
inline constexpr int kCjkSlot = 4;

/// One view owns at most 94 printable ASCII layouts per Latin font variant.
/// Explicit cell origins remain authoritative; natural text advances are unused.
class AsciiLayouts {
public:
    void clear() {
        if (!device_) return;
        for (auto &face : faces_) {
            for (auto &layout : face.layouts) layout.reset();
            face.font = QFont();
        }
        device_ = nullptr;
    }
    void begin(QPainter &painter, const Variants &fonts, qreal dpr) {
        QPaintDevice *device = painter.device();
        const bool changed = device != device_ || dpr != dpr_ ||
            device->logicalDpiX() != dpi_x_ || device->logicalDpiY() != dpi_y_;
        for (size_t i = 0; i < faces_.size(); ++i) {
            if (changed || faces_[i].font != fonts[i]) {
                for (auto &layout : faces_[i].layouts) layout.reset();
                faces_[i].font = fonts[i];
            }
        }
        device_ = device;
        dpr_ = dpr;
        dpi_x_ = device->logicalDpiX();
        dpi_y_ = device->logicalDpiY();
    }
    bool draw(QPainter &painter, const QPointF &at, quint32 cp, int font) {
        if (cp < 33 || cp > 126 || font < 0 || font >= int(faces_.size()) ||
            painter.layoutDirection() == Qt::RightToLeft) return false;
        auto &face = faces_[size_t(font)];
        auto &layout = face.layouts[size_t(cp - 33)];
        if (!layout) {
            layout = std::make_unique<QTextLayout>(QString(QChar(ushort(cp))), face.font, device_);
            layout->setCacheEnabled(true);
            layout->beginLayout();
            layout->createLine().setLineWidth(10000);
            layout->endLayout();
        }
        layout->draw(&painter, at - QPointF(0, layout->lineAt(0).ascent()));
        return true;
    }
private:
    struct Face {
        QFont font;
        std::array<std::unique_ptr<QTextLayout>, 94> layouts;
    };
    std::array<Face, 4> faces_;
    QPaintDevice *device_ = nullptr;
    qreal dpr_ = 0;
    int dpi_x_ = 0, dpi_y_ = 0;
};

/// 码位是否需要单族 CJK 字面：U+2E80 起是中日韩符号/假名/汉字/全角 forms 区。
/// 刻意**不**让 Qt 自己回退：一个 QFont 只要挂 ≥2 个族名、或用不含汉字的族名画汉字，
/// Qt 就会建出整套回退引擎链，一次性吃 ~40 MB（实测窗格 12.0 → 53.1 MB）。
inline bool needs_cjk_face(quint32 cp) {
    return cp >= 0x2E80u && cp < 0x1F000u; // 星平面（emoji）交给 Qt，极少走到
}

/// 颜色字 → QColor（字节序见文件头）。
inline QColor color_of(quint32 packed) {
    return QColor(int(packed & 0xFFu), int((packed >> 8) & 0xFFu), int((packed >> 16) & 0xFFu),
                  int((packed >> 24) & 0xFFu));
}

/// 画一整帧。`variants` 由视图缓存（仅字号/dpr 变化时重建）。
inline void paint(QPainter &p, const rust::Vec<uint32_t> &recs, qreal dpr,
                  const Variants &variants, AsciiLayouts &layouts, const QRectF &caret_line = QRectF(),
                  const QColor &caret_tint = QColor()) {
    const std::size_t n = recs.size() / kStride;
    if (n == 0 || dpr <= 0.0 || !p.isActive()) {
        return;
    }
    // Sparse prompts do not justify retaining shaped layouts. Dense frames
    // amortize their setup; stop counting once the bounded activation point is met.
    size_t ascii = 0;
    if (p.layoutDirection() != Qt::RightToLeft) {
        const uint32_t *record = recs.data();
        for (size_t i = 0; i < n && ascii < 128; ++i, record += kStride)
            if ((record[0] & 0xffu) == kKindText && record[4] >= 33 && record[4] <= 126) ++ascii;
    }
    const bool use_layouts = ascii >= 128;
    if (use_layouts) layouts.begin(p, variants, dpr);
    else layouts.clear();
    p.setBrush(Qt::NoBrush);
    const uint32_t *w = recs.data();
    // 同色缓存：记录按绘制顺序排列，相邻记录大量同色（整屏底色、成段正文），
    // 逐条构造 QColor 是纯开销。缓存必须用**独立的存在标志**，不能拿某个颜色值
    // 当哨兵 —— 原来用 0xFFFFFFFFu 当"还没设过"，而它正好是纯白 #FFFFFFFF：
    // Light 主题的底色就是它，于是首条记录命中哨兵、直接用默认构造的 QColor
    // （黑色）去 fillRect，整块终端正文被刷成黑色。
    quint32 last_color = 0;
    bool have_color = false;
    QColor col;
    int last_font = -1;

    for (std::size_t i = 0; i < n; ++i, w += kStride) {
        const quint32 kind = w[0] & 0xFFu;
        const quint32 flags = (w[0] >> 8) & 0xFFu;
        const qreal x = qreal(w[1] & 0xFFFFu) / dpr;
        const qreal y = qreal(w[1] >> 16) / dpr;
        if (!have_color || w[3] != last_color) {
            last_color = w[3];
            have_color = true;
            col = color_of(w[3]);
            p.setPen(col);
        }
        if (kind == kKindRect) {
            p.fillRect(QRectF(x, y, qreal(w[2] & 0xFFFFu) / dpr, qreal(w[2] >> 16) / dpr), col);
            // Place the line tint above the base background, below styled cells/text.
            if (i == 0 && !caret_line.isEmpty()) p.fillRect(caret_line, caret_tint);
            continue;
        }
        if (kind != kKindText) {
            continue; // 未知记录：忽略（桥面演进时旧壳不致画错）
        }
        const quint32 cp = w[4];
        const int font = needs_cjk_face(cp)
                             ? kCjkSlot
                             : int((flags & kFlagBold ? 2 : 0) | (flags & kFlagItalic ? 1 : 0));
        if (font != last_font) {
            p.setFont(variants[size_t(font)]);
            last_font = font;
        }
        const QPointF at(x, y);
        if (use_layouts && layouts.draw(p, at, cp, font)) continue;
        if (cp < 0x10000u) {
            p.drawText(at, QChar(ushort(cp)));
        } else {
            // 星平面：手工配对代理（滚动缓冲侧已把非 BMP 折叠成 U+FFFD，极少走到）。
            const quint32 v = cp - 0x10000u;
            p.drawText(at, QString(QChar(ushort(0xD800u + (v >> 10)))) + QChar(ushort(0xDC00u + (v & 0x3FFu))));
        }
    }
}

} // namespace tspans
