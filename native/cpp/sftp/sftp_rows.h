#pragma once

#include <QString>
#include <QtGlobal>

class QTreeWidgetItem;

namespace sftp_ui {

QString size_label(bool dir, quint64 bytes);
void fill_row(QTreeWidgetItem *item, const QString &name, bool dir, quint64 size,
              quint32 permissions, qint64 modified_unix, const QString &owner = QString(),
              const QString &group = QString());

} // namespace sftp_ui
