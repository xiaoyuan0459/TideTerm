// icon_cache 实现：全部图形用 QPainter 原语画在 14×14 逻辑方框里（WT 实测尺寸）。
//
// 命名与参考菜单逐项对应（终端/文件管理器/会话树三套菜单的图标并集，同形者共用名字）：
//   copy paste command star search clear_scroll screen_clear select_all prev next refresh
//   binary log reset delete rename move download upload new_dir new_file link path_term
//   info palette sync duplicate save split_h split_v close settings reopen
// 2026-09-25 快捷栏批次追加：add / menu / terminal（标签条右端簇与工具行用）。
#include "cpp/chrome/icon_cache.h"
#include "cpp/chrome/icon_shapes.h"

#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"

#include <QColor>
#include <QAction>
#include <QApplication>
#include <QHash>
#include <QIconEngine>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QPointF>
#include <QRectF>
#include <QPixmapCache>
#include <QStyle>
#include <QStyleOption>
#include <QAbstractButton>

#include <cmath>

namespace chrome {

namespace {

quint64 cache_generation = 0;

QHash<QString, QIcon> &faces() {
    static QHash<QString, QIcon> c;
    return c;
}

QPixmap render(const QString &name, int size, qreal dpr, const QColor &ink) {
    const int px = int(size * dpr + 0.5);
    QPixmap pm(px, px);
    pm.fill(Qt::transparent);
    QPainter p(&pm);
    p.setRenderHint(QPainter::Antialiasing, true);
    // 2026-10-06：按整数物理画布缩放（px/size），而不是 dpr——非整 dpr（如 1.25）
    // 下 14 逻辑 × 1.25 = 17.5 物理落在 18px 位图上，0.5px 缩放残差让笔画
    // 整体糊半档（用户反馈图标发虚）。改为 14 逻辑 → 18 物理整格映射。
    const qreal scale = qreal(px) / qreal(size);
    p.scale(scale, scale);
    // 字形边长：菜单档按 WT 实测取 14（画布 17 = 14 + 上下留白，QMenu 行高由画布决定）；
    // 其余调用方按请求尺寸画（页头 16、工具栏/列表 14）。
    const qreal glyph = size == metrics::kMenuIconCanvas ? metrics::kMenuIconSize : size;
    // 居中偏移也吸附到物理像素：非整偏移会把 paint_one 里的笔画对齐整体挪回半像素上。
    const qreal off = std::floor(((qreal(size) - glyph) / 2.0) * scale + 0.5) / scale;
    p.translate(off, off);
    detail::paint_icon_shape(p, name, glyph, ink, scale);
    p.end();
    pm.setDevicePixelRatio(dpr);
    return pm;
}

// QIcon asks for the actual size/DPI when painting; keep unused monitor scales out of memory.
class ChromeIconEngine final : public QIconEngine {
public:
    ChromeIconEngine(const QString &name, int size, const QColor &ink)
        : name_(name), size_(size), ink_(ink) {}
    QIconEngine *clone() const override { return new ChromeIconEngine(name_, size_, ink_); }
    bool isNull() override { return false; }
    QSize actualSize(const QSize &size, QIcon::Mode, QIcon::State) override {
        const int side = qMin(size_, qMin(size.width(), size.height()));
        return QSize(side, side);
    }
    QPixmap pixmap(const QSize &size, QIcon::Mode mode, QIcon::State state) override {
        return scaledPixmap(size, mode, state, 1.0);
    }
    QPixmap scaledPixmap(const QSize &size, QIcon::Mode mode, QIcon::State state, qreal dpr) override {
        const int side = actualSize(size, mode, state).width();
        if (side <= 0) return {};
        const QString key = QStringLiteral("TideTerm.chrome:") + name_ + QLatin1Char('@') +
                            QString::number(side) + QLatin1Char('x') + QString::number(dpr, 'g', 16) +
                            ink_.name(QColor::HexArgb) + QString::number(int(mode)) + QLatin1Char(':') +
                            QString::number(cache_generation);
        QPixmap pm;
        if (QPixmapCache::find(key, &pm)) return pm;
        pm = render(name_, side, dpr, ink_);
        if (mode != QIcon::Normal) {
            QStyleOption option;
            option.palette = QApplication::palette();
            pm = QApplication::style()->generatedIconPixmap(mode, pm, &option);
        }
        QPixmapCache::insert(key, pm);
        return pm;
    }
    void paint(QPainter *p, const QRect &rect, QIcon::Mode mode, QIcon::State state) override {
        const QSize size = actualSize(rect.size(), mode, state);
        const QPixmap pm = scaledPixmap(size, mode, state, p->device()->devicePixelRatioF());
        if (pm.isNull()) return;
        const QSizeF logical = pm.deviceIndependentSize();
        QPointF origin(rect.x() + (rect.width() - logical.width()) / 2,
                       rect.y() + (rect.height() - logical.height()) / 2);
        // Include the widget/backing-store translation, not just its local coordinates.
        // Keep the cached bitmap at one source pixel per destination pixel.
        const QTransform transform = p->deviceTransform();
        bool invertible = false;
        const QTransform inverse = transform.inverted(&invertible);
        if (invertible && transform.type() <= QTransform::TxScale) {
            const QPointF physical = transform.map(origin);
            origin = inverse.map(QPointF(qRound(physical.x()), qRound(physical.y())));
        }
        p->drawPixmap(origin, pm);
    }
private:
    QString name_;
    int size_;
    QColor ink_;
};

} // namespace

QIcon scalable_icon(const QString &name, int size, const QColor &ink) {
    const QColor color = ink.isValid() ? ink : ui_colors().text_secondary;
    const QString key = name + QString::number(size) + color.name(QColor::HexArgb);
    auto &c = faces();
    if (auto it = c.constFind(key); it != c.constEnd()) return it.value();
    const QIcon face(new ChromeIconEngine(name, size, color));
    if (c.size() >= 128) c.clear();
    c.insert(key, face);
    return face;
}

QString protocol_icon(const QString &protocol) {
    const QString value = protocol.toLower();
    if (value == QLatin1String("local") || value == QLatin1String("shell")) return QStringLiteral("terminal");
    if (value == QLatin1String("ssh")) return QStringLiteral("remote_host");
    if (value == QLatin1String("serial")) return QStringLiteral("serial");
    if (value == QLatin1String("tcp") || value == QLatin1String("telnet")) return QStringLiteral("network");
    return QStringLiteral("link");
}

/// SessCfg.kind（0=local 1=ssh 2=telnet 3=serial 4=tcp）→ 协议名，供
/// 只拿得到 kind 的调用点（如资源管理器 tab）复用同一张协议映射。
QString protocol_icon_for_kind(uint8_t kind) {
    switch (kind) {
        case 1: return QStringLiteral("remote_host");
        case 2: return QStringLiteral("network");
        case 3: return QStringLiteral("serial");
        case 4: return QStringLiteral("network");
        default: return QStringLiteral("terminal");
    }
}

void clear_icon_cache() { faces().clear(); ++cache_generation; }

void set_button_icon(QAbstractButton *button, const QString &name, int size, IconInk ink) {
    button->setProperty("chromeIcon", name);
    button->setProperty("chromeIconSize", size);
    // 记角色不记色值：QIcon 是位图缓存，换主题后 refresh_button_icons 要按新主题重解析。
    button->setProperty("chromeIconInkRole", int(ink));
    const QColor color = ink == IconInk::Primary ? ui_colors().text
        : ink == IconInk::Success ? ui_colors().status_connected : ui_colors().text_secondary;
    button->setIcon(scalable_icon(name, size, color));
    button->setIconSize(QSize(size, size));
}

void refresh_button_icons(QWidget *root) {
    for (auto *b : root->findChildren<QAbstractButton *>()) {
        const QString name = b->property("chromeIcon").toString();
        if (name.isEmpty()) continue;
        const auto role = IconInk(b->property("chromeIconInkRole").toInt());
        set_button_icon(b, name, b->property("chromeIconSize").toInt(), role);
    }
    for (auto *a : root->findChildren<QAction *>()) {
        const QString name = a->property("chromeIcon").toString();
        if (!name.isEmpty()) {
            const int size = a->property("chromeIconSize").toInt();
            a->setIcon(scalable_icon(name, size > 0 ? size : 14));
        }
    }
}

} // namespace chrome
