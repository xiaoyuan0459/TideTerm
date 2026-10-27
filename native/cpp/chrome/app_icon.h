#pragma once

class QApplication;

namespace chrome {

/// Install the packaged app icon, with a source-tree fallback for cargo run.
void install_app_icon(QApplication &app);

} // namespace chrome
