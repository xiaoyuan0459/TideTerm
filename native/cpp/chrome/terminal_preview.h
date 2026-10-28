#pragma once
#include <cstdint>
class MainWindow;
class PtyView;
namespace chrome {
// id == 0 previews the pane's selection; other IDs bind a retained command block.
void show_terminal_preview(MainWindow *window, PtyView *view, uint64_t command_id = 0);
}
