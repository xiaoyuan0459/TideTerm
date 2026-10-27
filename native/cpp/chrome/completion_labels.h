#pragma once
#include "cpp/i18n.h"
#include <QString>
#include <cstdint>

namespace chrome {
inline QString completion_shell_label(uint8_t shell) {
    switch (shell) {
    case 1: return QStringLiteral("Bash");
    case 2: return QStringLiteral("PowerShell");
    case 3: return QStringLiteral("cmd");
    case 4: return QStringLiteral("zsh");
    case 5: return QStringLiteral("fish");
    case 6: return QStringLiteral("POSIX sh");
    default: return i18n::tr("Shell 未确认");
    }
}
inline QString completion_origin_label(uint8_t kind) {
    switch (kind) {
    case 0: return i18n::tr("本终端历史");
    case 1: return i18n::tr("用户命令");
    case 2: return i18n::tr("参数规则");
    case 3: return i18n::tr("本地路径");
    case 4: return i18n::tr("SSH 路径");
    case 5: return i18n::tr("本地程序目录");
    case 6: return i18n::tr("SSH 程序目录");
    case 7: return i18n::tr("Shell 解析（显式查询）");
    case 8: case 9: return i18n::tr("Git 引用（显式快照）");
    case 10: return i18n::tr("Docker 容器（显式快照）");
    case 11: return i18n::tr("kubectl 资源（显式快照）");
    case 13: return i18n::tr("命令词库（参考）");
    default: return i18n::tr("Shell 名字目录（显式采集）");
    }
}
// The terminal uses a compact rail; full origins stay in accessible text/tooltips.
inline QString completion_origin_badge(uint8_t kind) {
    switch (kind) {
    case 0: return QStringLiteral("h");
    case 1: return QStringLiteral("u");
    case 3: case 4: case 5: case 6: return QStringLiteral("p");
    case 8: case 9: return QStringLiteral("g");
    case 10: return QStringLiteral("d");
    case 11: return QStringLiteral("k");
    default: return QStringLiteral("c");
    }
}
inline QString completion_system_label(uint8_t system) {
    switch (system) {
    case 1: return QStringLiteral("Windows");
    case 2: return QStringLiteral("Linux");
    case 3: return QStringLiteral("macOS");
    default: return i18n::tr("系统未确认");
    }
}
}
