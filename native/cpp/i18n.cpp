#include "cpp/i18n.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QLocale>
#include <QStringList>
#include <QXmlStreamReader>

#include <cstdlib>

namespace i18n {
namespace {

QHash<QString, QString> g_table;
QString g_locale;

// <message><source>…</source><translation>…</translation></message>
// 跳过 type="unfinished"/"vanished" 的 <translation>——未完成条目按缺失处理，
// 让界面回落中文，而不是展示半成品译文。
bool parseTs(const QByteArray &xml, QHash<QString, QString> *out) {
    QXmlStreamReader reader(xml);
    QString source;
    QString translation;
    QString elem;
    bool inMessage = false;
    bool unfinished = false;

    auto flush = [&]() {
        if (!source.isEmpty() && !translation.isEmpty() && !unfinished) {
            out->insert(source, translation);
        }
        source.clear();
        translation.clear();
        unfinished = false;
    };

    while (!reader.atEnd()) {
        const QXmlStreamReader::TokenType token = reader.readNext();
        if (token == QXmlStreamReader::StartElement) {
            const QStringView name = reader.name();
            if (name == QLatin1String("message")) {
                inMessage = true;
                flush();
            } else if (inMessage && (name == QLatin1String("source")
                                    || name == QLatin1String("translation"))) {
                elem = name.toString();
                if (name == QLatin1String("translation")) {
                    const QStringView type = reader.attributes().value(QLatin1String("type"));
                    unfinished = type == QLatin1String("unfinished")
                              || type == QLatin1String("vanished");
                }
            }
        } else if (token == QXmlStreamReader::Characters) {
            if (inMessage && !elem.isEmpty()) {
                if (elem == QLatin1String("source")) {
                    source += reader.text().toString();
                } else {
                    translation += reader.text().toString();
                }
            }
        } else if (token == QXmlStreamReader::EndElement) {
            const QStringView name = reader.name();
            if (name == QLatin1String("message")) {
                flush();
                inMessage = false;
            } else if (inMessage && name == elem) {
                elem.clear();
            }
        }
    }
    return !reader.hasError();
}

} // namespace

QString translationsDir() {
    // 发布布局：<exe>/i18n/tideterm_xx.ts（由安装/打包步骤带上）。
    const QString shipped = QCoreApplication::applicationDirPath() + QLatin1String("/i18n");
    if (QFileInfo::exists(shipped)) {
        return shipped;
    }
    // 开发布局：在 target/<profile>/ 下直接跑（自检、cargo run）时翻译表还没
    // 被拷过去。从 exe 目录逐级上溯找源码树里的 native/i18n，不依赖 CWD。
    QDir dir(QCoreApplication::applicationDirPath());
    for (int depth = 0; depth < 6; ++depth) {
        if (!dir.cdUp()) {
            break;
        }
        const QString candidate = dir.absolutePath() + QLatin1String("/native/i18n");
        if (QFileInfo::exists(candidate)) {
            return candidate;
        }
    }
    // 都找不到：返回 shipped，调用方按"翻译缺失"处理（界面回落中文）。
    return shipped;
}

bool install(const QString &locale) {
    g_table.clear();
    g_locale.clear();
    // 源语言（空串 / zh_CN / zh*）：清空即回落中文。
    if (locale.isEmpty() || locale.startsWith(QLatin1String("zh"))) {
        return false;
    }
    const QString path = translationsDir() + QLatin1String("/tideterm_")
                       + locale.left(2) + QLatin1String(".ts");
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return false;
    }
    if (!parseTs(file.readAll(), &g_table) || g_table.isEmpty()) {
        g_table.clear();
        return false;
    }
    g_locale = locale;
    return true;
}

bool install_from_env() {
    // 显式环境变量优先（自检/脚本可强制指定），否则跟随系统 locale。
    const QByteArray env = qgetenv("TIDETERM_LANG");
    if (!env.isEmpty()) {
        return install(QString::fromUtf8(env));
    }
    const QString system = QLocale::system().name(); // 形如 zh_CN / en_US
    return install(system);
}

QString tr(const char *source) {
    if (g_table.isEmpty()) {
        return QString::fromUtf8(source);
    }
    const QString key = QString::fromUtf8(source);
    const auto it = g_table.constFind(key);
    return it == g_table.constEnd() ? key : it.value();
}

QString tr(const QString &source) {
    if (g_table.isEmpty()) {
        return source;
    }
    const auto it = g_table.constFind(source);
    return it == g_table.constEnd() ? source : it.value();
}

const QString &currentLocale() {
    return g_locale;
}

int loadedCount() {
    return g_table.size();
}

} // namespace i18n
