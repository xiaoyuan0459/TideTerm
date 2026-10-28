#include "cpp/chrome/file_icons.h"
#include "cpp/i18n.h"
#include <QCache>
#include <QFileIconProvider>
#include <QFileInfo>
#include <QImage>
#include <QPixmap>
#ifdef Q_OS_WIN
#include <qt_windows.h>
#include <shellapi.h>
#endif

namespace chrome {
namespace {
struct FileType { QIcon icon; QString label; };
FileType type(const QString &name, bool directory) {
    static QCache<QString, FileType> cache(128);
    const QString suffix = QFileInfo(name).suffix().toLower();
    const QString key = i18n::currentLocale() + QLatin1Char('/')
        + (directory ? QStringLiteral("<directory>") : suffix);
    if (const auto *cached = cache.object(key)) return *cached;
    FileType value;
#ifdef Q_OS_WIN
    // USEFILEATTRIBUTES retrieves the system association without opening the file,
    // following .lnk targets, accessing network shares or decoding thumbnails.
    const QString sample = directory ? QStringLiteral("folder") : QStringLiteral("file.") + suffix;
    SHFILEINFOW info{};
    if (SHGetFileInfoW(reinterpret_cast<LPCWSTR>(sample.utf16()),
        directory ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL, &info, sizeof(info),
        SHGFI_ICON | SHGFI_LARGEICON | SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES)) {
        value.icon = QIcon(QPixmap::fromImage(QImage::fromHICON(info.hIcon)));
        value.label = QString::fromWCharArray(info.szTypeName);
        DestroyIcon(info.hIcon);
    }
#endif
    if (value.icon.isNull()) {
        QFileIconProvider provider;
        value.icon = provider.icon(directory ? QFileIconProvider::Folder : QFileIconProvider::File);
    }
    if (value.label.isEmpty() || !i18n::currentLocale().isEmpty()) {
        value.label = directory ? i18n::tr("目录") :
            (suffix.isEmpty() ? i18n::tr("文件") : i18n::tr("%1 文件").arg(suffix.toUpper()));
    }
    cache.insert(key, new FileType(value));
    return value;
}
}
QIcon file_type_icon(const QString &name, bool directory) { return type(name, directory).icon; }
QString file_type_label(const QString &name, bool directory) { return type(name, directory).label; }
}
