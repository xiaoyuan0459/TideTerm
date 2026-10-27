// prompts —— SSH 交互提示轮询与应答（Phase C ④）。
//
// 主机密钥确认（安全铁律：指纹必须外显，KeyChanged 醒目警示，绝不静默接受）
// 与 keyboard-interactive 挑战（多字段，echo=false 为密码框）。60s 超时由 Rust
// 侧 EventHostKeyInquirer 兜底自动拒绝。
#pragma once

#include "cpp/bridge_shim.h" // PromptInfo（session 域桥 POD）

class MainWindow;
class QWidget;

/// 轮询一条待应答提示并弹窗（MainWindow pump tick 调用；无待处理 = 空操作）。
void poll_prompts(MainWindow *win);

/// host key 确认对话框（独立函数供自检直接构造断言）。
/// 返回用户选择：true = 信任并记录，false = 拒绝。
bool run_host_key_dialog(QWidget *parent, const PromptInfo &p);
