// PrefsDialog —— 首选项对话框（Phase C ⑦；对齐 Slint 壳首选项三页语义）：
// 常规（主题/恢复会话）/ 终端（光标/行号/回滚预算）/ SSH（超时/host key）。
// OK = prefs_set 落盘 + 主题 QSS 全局应用。无 moc（std::function 回调）。
#pragma once

class MainWindow;

/// 打开首选项对话框（Phase C ⑦；对齐 Slint 壳首选项三页语义：常规/终端/SSH）。
/// focus_page：0=常规 1=终端 2=SSH（菜单三入口定位同一对话框到对应页）。
/// OK = prefs_set 落盘 + 主题 QSS 全局应用。返回 true = 已保存（调用方随后
/// 可 note 反馈）。无 moc（std::function/lambda 连接）。
bool run_prefs_dialog(MainWindow *win, int focus_page);
