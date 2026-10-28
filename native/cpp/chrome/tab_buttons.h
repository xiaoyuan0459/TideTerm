// Terminal tab titles and status/close widgets, shared across tab groups.
#pragma once

class MainWindow;

namespace chrome {

/// Number repeated instances of a saved session across windows, never page positions.
void sync_tab_titles();
/// 同步全部标签的角标（数量/状态/活动态）。
void sync_tab_buttons(MainWindow *win);

} // namespace chrome
