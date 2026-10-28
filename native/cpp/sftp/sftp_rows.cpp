#include "cpp/sftp/sftp_rows.h"
#include "cpp/i18n.h"

#include "cpp/chrome/file_icons.h"

#include <QDateTime>
#include <QTreeWidgetItem>

namespace sftp_ui {

// WT 实测（2026-09-29，25000 B → 「25 KB」、4400×25000 → 「110 MB」）：单位为
// 1000 进制；≥10 取整、<10 一位小数；目录大小列为空白（不是 "--"）。
QString size_label(bool dir, quint64 bytes) {
    if (dir) return QString();
    const char *units[] = {"B", "KB", "MB", "GB", "TB"};
    double value = double(bytes);
    int unit = 0;
    while (value >= 1000.0 && unit < 4) {
        value /= 1000.0;
        ++unit;
    }
    return value >= 10 || unit == 0
               ? QStringLiteral("%1 %2").arg(int(value)).arg(QString::fromLatin1(units[unit]))
               : QStringLiteral("%1 %2").arg(value, 0, 'f', 1).arg(QString::fromLatin1(units[unit]));
}

} // namespace sftp_ui

namespace {

QString permission_label(bool dir, quint32 permissions) {
    QString label = dir ? QStringLiteral("d") : QStringLiteral("-");
    const quint32 bits[] = {0400, 0200, 0100, 040, 020, 010, 04, 02, 01};
    const char chars[] = {'r', 'w', 'x'};
    for (int i = 0; i < 9; ++i)
        label += QChar((permissions & bits[i]) != 0 ? chars[i % 3] : '-');
    return label;
}

} // namespace

namespace sftp_ui {

void fill_row(QTreeWidgetItem *item, const QString &name, bool dir, quint64 size,
              quint32 permissions, qint64 modified_unix, const QString &owner,
              const QString &group) {
    item->setText(0, name);
    item->setIcon(0, chrome::file_type_icon(name, dir));
    item->setText(1, modified_unix > 0
                         ? QDateTime::fromSecsSinceEpoch(modified_unix).toString(QStringLiteral("yyyy/MM/dd HH:mm:ss"))
                         : QString());
    // WT 实测：远程树目录类型列 = 「目录」（Windows 机制给「文件夹」，仅本地栏适用）。
    item->setText(2, dir ? i18n::tr("目录") : chrome::file_type_label(name, false));
    item->setText(3, size_label(dir, size));
    item->setText(4, permission_label(dir, permissions));
    // 列 5/6 = 所有者/分组（远程树 7 列；本地 4 列树忽略——调用方仅远程走 fill_row）。
    item->setText(5, owner);
    item->setText(6, group);
    item->setTextAlignment(3, Qt::AlignRight | Qt::AlignVCenter);
    item->setData(0, Qt::UserRole, name);
    item->setData(0, Qt::UserRole + 1, dir);
    item->setData(0, Qt::UserRole + 2, size);
}

} // namespace sftp_ui
