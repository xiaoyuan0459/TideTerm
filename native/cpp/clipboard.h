#pragma once
#include <QString>

namespace host_clipboard {
// GUI thread only. Returns false if the host could not publish the text.
bool copy_text(const QString &text);
QString read_text();
}
