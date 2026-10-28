// C++ 视图层对外接口（Rust 侧经 cxx 1.0 调用）。
//
// 桥面铁律（ADR-012 §Decision 2）：
//   1. C++ 只做视图，不持有业务状态
//   2. 只传 POD（RawSurface / Rect / TermStats / 基本类型），不传 Rust trait
//   3. 不用 QTextDocument / QTextEdit / QPlainTextEdit 承载终端或日志正文
#pragma once

#include <cstdint>
#include "rust/cxx.h"
bool qt_clipboard_copy(rust::Str text);
rust::String qt_clipboard_read();
bool qt_clipboard_has_image();
bool qt_has_gui_application();

// cxx 生成的声明头（含 rust::Str / rust::String 与共享 POD）
#include "native/src/main.rs.h"
#include "native/src/bridge/context.rs.h"
#include "native/src/bridge/commands.rs.h"
#include "native/src/bridge/marks.rs.h"
#include "native/src/bridge/palette.rs.h"
#include "native/src/bridge/multiline.rs.h"
#include "native/src/bridge/completion.rs.h"
#include "native/src/bridge/files.rs.h"
#include "native/src/bridge/modem.rs.h"
#include "native/src/bridge/reading.rs.h"
#include "native/src/bridge/selection.rs.h"
#include "native/src/bridge/workspace.rs.h"
#include "native/src/bridge/sync.rs.h"
// pod 域桥（ADR-014 按域拆桥；Phase C ③ chrome 会话快照）
#include "native/src/bridge/pod.rs.h"
// session 域桥（Phase C ④ 会话持久化/表单/真实连接/交互提示）
#include "native/src/bridge/session.rs.h"
// sftp 域桥（Phase C ⑤ 文件管理器/传输）
#include "native/src/bridge/sftp.rs.h"
// ai 域桥（Phase C ⑥ 助手面板/配置）
#include "native/src/bridge/ai.rs.h"
// prefs 域桥（Phase C ⑦ 偏好/主题 QSS）
#include "native/src/bridge/prefs.rs.h"
// log 域桥（Phase C ⑧ 会话日志开关/导出）
#include "native/src/bridge/log.rs.h"
#include "native/src/bridge/onekey.rs.h"
// vault 域桥（主密码凭据库：设置/解锁/存取/会话清理）
#include "native/src/bridge/vault.rs.h"

/// C++ 侧上报的 Qt 版本（双向调用链证据）。
rust::String qt_version();

/// 脚手架假引擎自检（--selftest）：同步渲染 frames 帧并落盘 PNG。返回 0 = 成功。
int qt_boot(bool selftest, uint32_t frames, rust::Str png_out);
int qt_memory_probe(uint8_t stage);
int qt_boot_welcome(rust::Str png_out);

/// 真实引擎自检（G1）：本地 PTY + 喂 feed_cmd + 20ms 会话泵 + paintEvent 渲染
/// + 离屏 PNG + Rust 侧屏文本断言。offscreen 平台即可运行。
int qt_boot_pty(rust::Str feed_cmd, rust::Str png_out);

/// G2 IME 自检：合成 QInputMethodEvent（预编辑 + 提交）走真实 Qt IME 分发路径，
/// 断言组合窗锚定（ImCursorRectangle = 引擎光标格）与提交串经 PTY 上屏。
int qt_boot_g2(rust::Str png_out);

/// G3 系数试点：Qt 版 SFTP 浏览器离屏构建 + 结构断言 + PNG。
int qt_boot_g3(rust::Str png_out);

/// Phase C ①：多标签窗口自检（隔离/生命周期/标题）。
int qt_boot_c1(rust::Str png_out);

/// Phase C ②：分割/关窗格自检。
int qt_boot_c2(rust::Str png_out);
int qt_boot_reading(rust::Str png_out);
int qt_boot_history(rust::Str png_out);
int qt_boot_commands(rust::Str png_out);
int qt_boot_selection(rust::Str png_out);
int qt_boot_workspace(rust::Str png_out);

/// Phase C ③：chrome 骨架自检（菜单/横幅/发送栏/dock/树模型）。
int qt_boot_c3(rust::Str png_out);

/// Phase C ④：会话树+表单+快速连接自检（store hermetic 重定向 + CRUD 往返 +
/// 真实本地连接 + prompt 轮询）。
int qt_boot_c4(rust::Str png_out);

/// Phase C ⑤：SFTP+传输自检（浏览器结构/本地栏真扫/桥异步错误路径/传输队列）。
int qt_boot_c5(rust::Str png_out);

/// Phase C ⑥：AI 面板自检（面板结构/配置往返/异步失败路径）。
int qt_boot_c6(rust::Str png_out);

/// Phase C ⑦：偏好+主题自检（主题清单/QSS 应用/偏好往返/菜单首选项分发）。
int qt_boot_c7(rust::Str png_out);

/// Phase C ⑧：状态栏/日志自检（右字段组装/日志开关/导出空环语义）。
int qt_boot_c8(rust::Str png_out);
/// 对齐批次 A/B：webview 度量自检（main_window_layout/selftest_metrics.cpp）。
int qt_boot_c9(rust::Str png_out);

/// i18n 自检（--i18n-selftest）：翻译表加载/命中/回落三态，无参、offscreen 可跑。
int qt_i18n_selftest();

/// Phase C ①：交互主窗口（窗口+标签+单终端）。
int qt_window(rust::Str profile);

/// 交互模式（真窗口 + 键盘 + 鼠标 + IME + 20ms 会话泵；阻塞到窗口关闭）。
int qt_run_pty();
