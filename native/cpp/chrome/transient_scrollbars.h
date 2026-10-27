#pragma once

class QAbstractScrollArea;

namespace chrome {
// Overlay scrollbars preserve the viewport width as they appear and disappear.
void install_transient_scrollbars(QAbstractScrollArea *view);
}
