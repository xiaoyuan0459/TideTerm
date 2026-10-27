#pragma once

#include <QStringList>

class MainWindow;

/// Open the most recent saved session that still exists; leave the welcome page otherwise.
bool restore_saved_session(MainWindow *window, const QStringList &ids);
