// Phase C ③④ 自检入口（从 main.cpp 拆出——行数门禁）。
#pragma once

#include "cpp/bridge_shim.h"

/// Phase C ③：chrome 骨架自检（菜单/横幅/发送栏/dock/树模型）。
int qt_boot_c3(rust::Str png_out);

/// Phase C ④：会话树+表单+快速连接自检（store hermetic 重定向 + CRUD 往返 +
/// 真实本地连接 + prompt 轮询）。
int qt_boot_c4(rust::Str png_out);

/// Phase C ⑤：SFTP+传输自检（浏览器结构/本地栏真扫/桥异步错误路径/传输队列）。
int qt_boot_c5(rust::Str png_out);
