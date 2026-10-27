// Developer visual proof: uses the exact production QPainter geometry, no image library.
#include "cpp/chrome/icon_shapes.h"
#include <QDir>
#include <QFile>
#include <QFont>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QImage>
#include <QPainter>
#include <QTextStream>
#include <cstdio>

int main(int argc, char **argv) {
    QGuiApplication app(argc, argv);
    const int font_id = QFontDatabase::addApplicationFont(QStringLiteral("C:/Windows/Fonts/msyh.ttc"));
    const QStringList families = QFontDatabase::applicationFontFamilies(font_id);
    if (families.isEmpty()) return 6;
    if (argc != 3) return 2;
    QFile catalogue(QString::fromLocal8Bit(argv[1]));
    if (!catalogue.open(QIODevice::ReadOnly | QIODevice::Text)) return 3;
    QTextStream input(&catalogue);
    input.readLine(); // Header.
    QList<QStringList> entries;
    while (!input.atEnd()) {
        auto row = input.readLine().split(QLatin1Char('\t'));
        if (row.size() == 3) entries.append(row);
    }
    const QString output = QString::fromLocal8Bit(argv[2]);
    if (!QDir().mkpath(output)) return 4;
    for (bool light : {false, true}) {
        const QColor bg(light ? "#fafafa" : "#1e1e1e");
        const QColor ink(light ? "#444444" : "#d4d4d4");
        const QColor muted(light ? "#666666" : "#a0a0a0");
        constexpr int columns = 5, cell_width = 280, cell_height = 112, header = 90;
        QImage sheet(columns * cell_width, header + ((entries.size() + columns - 1) / columns) * cell_height,
                     QImage::Format_ARGB32_Premultiplied);
        sheet.fill(bg);
        QPainter p(&sheet);
        p.setRenderHint(QPainter::Antialiasing);
        QFont font(families.first());
        font.setPixelSize(22);
        p.setFont(font);
        p.setPen(ink);
        p.drawText(22, 32, QStringLiteral("TideTerm  ·  %1 个语义图标").arg(entries.size()));
        font.setPixelSize(12);
        p.setFont(font);
        p.setPen(muted);
        p.drawText(22, 60, QStringLiteral("独立 QPainter 矢量  |  大图 32px  |  小图 14px @ 100% / 125% / 150% / 200%（物理像素原尺寸）"));
        for (int i = 0; i < entries.size(); ++i) {
            const auto &row = entries[i];
            const int x = (i % columns) * cell_width, y = header + (i / columns) * cell_height;
            p.save();
            p.translate(x + 18, y + 10);
            chrome::detail::paint_icon_shape(p, row[0], 32, ink, 1);
            p.restore();
            p.setPen(ink);
            font.setPixelSize(14);
            p.setFont(font);
            p.drawText(x + 62, y + 22, row[1]);
            font.setPixelSize(11);
            p.setFont(font);
            p.setPen(muted);
            p.drawText(x + 62, y + 40, row[0]);
            int px = x + 62;
            for (qreal dpr : {1.0, 1.25, 1.5, 2.0}) {
                QImage glyph(qCeil(14 * dpr), qCeil(14 * dpr), QImage::Format_ARGB32_Premultiplied);
                glyph.fill(Qt::transparent);
                QPainter g(&glyph);
                g.setRenderHint(QPainter::Antialiasing);
                g.scale(dpr, dpr);
                chrome::detail::paint_icon_shape(g, row[0], 14, ink, dpr);
                g.end();
                p.drawImage(px, y + 49, glyph);
                px += 39;
            }
            p.drawText(QRect(x + 18, y + 83, cell_width - 30, 22), row[2]);
            p.setPen(light ? QColor("#e0e0e0") : QColor("#333333"));
            p.drawLine(x + 18, y + cell_height - 1, x + cell_width - 18, y + cell_height - 1);
        }
        p.end();
        const QString path = output + (light ? QStringLiteral("/icons-light.png") : QStringLiteral("/icons-dark.png"));
        if (!sheet.save(path)) return 5;
        std::printf("%s: %d icons\n", qPrintable(path), int(entries.size()));
    }
    return 0;
}
