#pragma once
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/icon_cache.h"
#include <QString>
#include <QByteArray>

namespace chrome {
inline QString shell_display_name(const QString &command) {
    const QByteArray value = command.toUtf8();
    const auto display = context_shell_display_name(rust::Str(value.constData(), size_t(value.size())));
    return QString::fromUtf8(display.data(), int(display.size()));
}
inline QString protocol_display_name(int kind) {
    if (kind == 3) return i18n::tr("串口");
    static const char *names[] = {"Shell", "SSH", "Telnet", "Serial", "TCP"};
    return QString::fromLatin1(names[qBound(0, kind, 4)]);
}
inline QString shell_menu_name(const QString &command) {
    const QString name = shell_display_name(command);
    return command.compare(QStringLiteral("pwsh"), Qt::CaseInsensitive) == 0
        ? name + QStringLiteral(" (pwsh)") : name;
}
inline QString shell_icon(const QString &command) {
    const QString name = shell_display_name(command);
    if (name == QStringLiteral("PowerShell")) return QStringLiteral("shell_powershell");
    if (name == QStringLiteral("Command Prompt")) return QStringLiteral("shell_cmd");
    if (name == QStringLiteral("Bash") || name == QStringLiteral("Git Bash")) return QStringLiteral("shell_bash");
    if (name == QStringLiteral("Zsh")) return QStringLiteral("shell_zsh");
    if (name == QStringLiteral("Fish")) return QStringLiteral("shell_fish");
    return QStringLiteral("terminal");
}
inline QString session_icon(const SessCfg &cfg) {
    if (cfg.kind != 0) return protocol_icon_for_kind(cfg.kind);
    const auto &value = cfg.local_path.empty() ? cfg.local_shell : cfg.local_path;
    QString shell = QString::fromUtf8(value.data(), int(value.size()));
    if (shell.isEmpty() || shell == QStringLiteral("auto")) {
#ifdef Q_OS_WIN
        shell = QStringLiteral("powershell");
#else
        shell = qEnvironmentVariable("SHELL", QStringLiteral("/bin/bash"));
#endif
    }
    return shell_icon(shell);
}
} // namespace chrome
