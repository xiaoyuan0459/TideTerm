#include "cpp/chrome/app_font.h"

#include <QApplication>
#include <QFontDatabase>
#include <QWidget>
#include "cpp/bridge_shim.h"

namespace chrome {
void init_app_font() {
#ifdef Q_OS_WIN
    static bool initialized = false;
    if (initialized) return;
    initialized = true;
    QString family = QStringLiteral("Microsoft YaHei UI");
    // 生产启动避免枚举整套系统字体；离屏插件缺少系统字体回退时才显式加载。
    if (QApplication::platformName() == QStringLiteral("offscreen")) {
        const int id = QFontDatabase::addApplicationFont(
            QStringLiteral("C:/Windows/Fonts/msyh.ttc"));
        if (id >= 0) {
            const QStringList names = QFontDatabase::applicationFontFamilies(id);
            if (!names.isEmpty()) family = names.first();
        }
    }
    QFont font(family);
    font.setPixelSize(13);
    // Let Windows use its configured text smoothing on opaque UI backgrounds.
    font.setStyleStrategy(QFont::PreferAntialias);
    font.setHintingPreference(QFont::PreferFullHinting);
    QApplication::setFont(font);
#endif
}
/// UI 正文字体（与 init_app_font 同族同号）+ 灰度抗锯齿：横幅文字/地址栏
/// 用——终端等宽脸（set_connection_font）没有中文字形，回退渲染发虚不端正
/// （2026-10-06 用户反馈「快速连接栏字体不够端正」）。
void set_ui_text_font(QWidget *widget) {
    QFont font = QApplication::font();
    font.setStyleStrategy(QFont::StyleStrategy(QFont::PreferAntialias | QFont::NoSubpixelAntialias));
    widget->setFont(font);
}

void set_connection_font(QWidget *widget) {
    const auto family = term_font_family();
    QFont font(QString::fromUtf8(family.data(), int(family.size())));
    font.setPixelSize(13);
    font.setStyleStrategy(QFont::PreferAntialias);
    font.setHintingPreference(QFont::PreferFullHinting);
    widget->setFont(font);
}
} // namespace chrome
