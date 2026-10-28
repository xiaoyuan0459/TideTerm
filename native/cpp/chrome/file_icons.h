#pragma once
#include <QIcon>
#include <QString>
namespace chrome {
// Cached type associations, with no per-file shortcut/thumbnail extraction.
QIcon file_type_icon(const QString &name, bool directory);
QString file_type_label(const QString &name, bool directory);
}
