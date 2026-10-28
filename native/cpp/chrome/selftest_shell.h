#pragma once
#include <QString>
#include <string_view>

namespace selftest_shell {
inline bool ready(std::string_view text) {
#ifdef Q_OS_WIN
    return text.find('>') != std::string_view::npos;
#else
    return text.find_first_of("#$%") != std::string_view::npos;
#endif
}
inline QString cd(QString path) {
#ifdef Q_OS_WIN
    return QStringLiteral("Set-Location -LiteralPath '%1'").arg(path.replace(QLatin1Char(39), QStringLiteral("''")));
#else
    return QStringLiteral("cd -- '%1'").arg(path.replace(QLatin1Char(39), QStringLiteral("'\"'\"'")));
#endif
}
inline QString history_command() {
#ifdef Q_OS_WIN
    return QStringLiteral("Write-Output 'HISTORY_DIRECT'");
#else
    return QStringLiteral("printf '%s\\n' 'HISTORY_DIRECT'");
#endif
}
inline QString cwd_witness(QString path) {
#ifdef Q_OS_WIN
    return QStringLiteral("[IO.File]::WriteAllText('%1', $PWD.ProviderPath, [Text.UTF8Encoding]::new($false))")
        .arg(path.replace(QLatin1Char(39), QStringLiteral("''")));
#else
    return QStringLiteral("printf '%s' \"$PWD\" > '%1'")
        .arg(path.replace(QLatin1Char(39), QStringLiteral("'\"'\"'")));
#endif
}
}
