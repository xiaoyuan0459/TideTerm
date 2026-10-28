#pragma once

class MainWindow;
namespace chrome {
void install_window_frame(MainWindow *window);
void open_quick_connect(MainWindow *window);
void focus_quick_connect(MainWindow *window);
// Hiding menu entries keeps the integrated caption and its controls usable.
void set_main_menus_visible(MainWindow *window, bool visible);
}
