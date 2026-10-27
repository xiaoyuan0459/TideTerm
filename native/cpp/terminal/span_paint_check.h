#pragma once
// Pixel oracle: explicit QPainter drawText origins, independent of layout caching.
#include "cpp/terminal/span_paint.h"
#include <QDir>
#include <QFileInfo>
#include <QFontDatabase>
#include <QImage>
#include <QRawFont>
#include <vector>

namespace span_paint_check {
inline void prepare_fonts() {
#ifdef Q_OS_WIN
    for (const char *path : {"C:/Windows/Fonts/CascadiaMono.ttf", "C:/Windows/Fonts/consola.ttf", "C:/Windows/Fonts/msyh.ttc"})
        if (QFileInfo::exists(QString::fromLatin1(path))) QFontDatabase::addApplicationFont(QString::fromLatin1(path));
#endif
}

inline bool verify(const QString &evidence) {
    struct Command {
        quint32 cp;
        int font, x, y, w, h;
        QColor color;
    };
    tspans::AsciiLayouts cache;
    QImage actual(1400, 420, QImage::Format_ARGB32_Premultiplied);
    QImage expected(actual.size(), actual.format());
    int cases = 0, matched = 0;
    bool saved = false;
    for (const QString &family : {QStringLiteral("Cascadia Mono"), QStringLiteral("Consolas"), QApplication::font().family()}) {
        for (int px : {14, 18, 22}) for (qreal dpr : {1.0, 119.0 / 96.0, 1.25, 2.0}) {
            tspans::Variants fonts;
            for (int i = 0; i < 5; ++i) {
                fonts[size_t(i)] = QFont(i == 4 ? QApplication::font().family() : family);
                fonts[size_t(i)].setPixelSize(px);
                fonts[size_t(i)].setBold(i < 4 && (i & 2));
                fonts[size_t(i)].setItalic(i < 4 && (i & 1));
                fonts[size_t(i)].setStyleStrategy(QFont::StyleStrategy(QFont::PreferAntialias | QFont::NoSubpixelAntialias));
                fonts[size_t(i)].setHintingPreference(QFont::PreferFullHinting);
            }
            if (!QRawFont::fromFont(fonts[0]).supportsCharacter(uint('M'))) {
                std::printf("[span-layout] real ASCII font unavailable\n"); return false;
            }
            for (int phase = 0; phase < 6; ++phase) {
                for (auto &font : fonts) font.setPixelSize(px + (phase == 2 ? 1 : 0));
                actual.setDevicePixelRatio(dpr); expected.setDevicePixelRatio(dpr);
                const int dots = phase == 3 ? 4724 : 3780;
                actual.setDotsPerMeterX(dots); actual.setDotsPerMeterY(dots);
                expected.setDotsPerMeterX(dots); expected.setDotsPerMeterY(dots);
                actual.fill(Qt::transparent); expected.fill(Qt::transparent);
                const QColor background(0x16, 0x16, 0x1e);
                std::vector<Command> commands{{0, 0, 0, 0, 1400, 420, background}};
                for (int row = 0; row < 4; ++row) for (int col = 0; col < (phase == 1 ? 8 : 94); ++col) {
                    const QColor color = (col + phase) % 3 ? QColor(0xc0, 0xca, 0xf5) : QColor(0x70, 0xc0, 0x80, 180);
                    commands.push_back({quint32(33 + col), row, col * 12 + phase, row * 38 + 34, 0, 0, color});
                }
                commands.push_back({0x4e2d, 4, 15, 210, 0, 0, QColor(210, 160, 100)});
                commands.push_back({0x1f642, 0, 55, 210, 0, 0, QColor(210, 160, 100)});
                commands.push_back({0, 0, 90, 192, 12, 24, QColor(90, 100, 170)});
                commands.push_back({'A', 2, 90, 210, 0, 0, background});
                rust::Vec<uint32_t> records;
                for (const auto &command : commands) {
                    const auto &c = command.color;
                    const quint32 flags = command.font < 4 ? quint32((command.font & 2 ? 1 : 0) | (command.font & 1 ? 2 : 0)) : 0;
                    records.push_back(command.cp ? 1u | (flags << 8) : 0u);
                    records.push_back(quint32(command.x) | (quint32(command.y) << 16));
                    records.push_back(quint32(command.w) | (quint32(command.h) << 16));
                    records.push_back(quint32(c.red()) | (quint32(c.green()) << 8) | (quint32(c.blue()) << 16) | (quint32(c.alpha()) << 24));
                    records.push_back(command.cp); records.push_back(0);
                }
                {
                    QPainter painter(&expected);
                    painter.setRenderHint(QPainter::TextAntialiasing);
                    painter.setLayoutDirection(phase == 4 ? Qt::RightToLeft : Qt::LeftToRight);
                    painter.translate(15 / dpr, 10 / dpr);
                    for (const auto &command : commands) {
                        painter.setPen(command.color);
                        if (!command.cp) {
                            painter.fillRect(QRectF(command.x / dpr, command.y / dpr, command.w / dpr, command.h / dpr), command.color);
                        } else {
                            painter.setFont(fonts[size_t(command.font)]);
                            const char32_t cp = char32_t(command.cp);
                            painter.drawText(QPointF(command.x / dpr, command.y / dpr), QString::fromUcs4(&cp, 1));
                        }
                    }
                }
                {
                    QPainter painter(&actual);
                    painter.setRenderHint(QPainter::TextAntialiasing);
                    painter.setLayoutDirection(phase == 4 ? Qt::RightToLeft : Qt::LeftToRight);
                    painter.translate(15 / dpr, 10 / dpr);
                    tspans::paint(painter, records, dpr, fonts, cache);
                }
                ++cases;
                if (actual == expected) ++matched;
                else std::printf("[span-layout] pixel mismatch px=%d dpr=%.6f phase=%d\n", px, double(dpr), phase);
                if (!saved && px == 14 && qAbs(dpr - 119.0 / 96.0) < 0.0001 && phase == 1 && !evidence.isEmpty()) {
                    expected.save(evidence + QStringLiteral(".reference.png"));
                    actual.save(evidence + QStringLiteral(".cached.png"));
                    saved = true;
                }
            }
        }
    }
    std::printf("[span-layout] pixel-equivalent=%d/%d dense/sparse/font/dpr/recolor/order/rtl=%d\n", matched, cases, matched == cases);
    return cases == 216 && matched == cases;
}
}
