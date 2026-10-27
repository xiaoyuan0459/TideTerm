#pragma once

class QAction;
class MainWindow;
class QString;

namespace chrome {
bool dispatch_extra(MainWindow *win, QAction *action);
void record_recent(const QString &id);
}
