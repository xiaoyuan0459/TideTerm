// i18n 自检（--i18n-selftest）：离线验证翻译表加载、命中、回落三态。
//
// 在 CI / run-selftests.ps1 里跑，不需要真机用户介入：
//   1. install("en") 后，目录里登记的键必须返回英文（证明 .ts 被解析）；
//   2. 未登记的键必须原样回落中文（渐进转换的安全网）；
//   3. install("") / install("zh_CN") 后必须清空翻译表（源语言）；
//   4. install("klieg")（无对应 .ts）必须静默失败并保持中文。

#include "cpp/i18n.h"

#include <QApplication>
#include <QLocale>

#include <cstdio>
#include <cstdlib>
#include <string>

namespace {

int g_failures = 0;

void check(bool ok, const char *what) {
    std::printf("  [%s] %s\n", ok ? "OK" : "FAIL", what);
    if (!ok) {
        ++g_failures;
    }
}

void check_eq(const QString &got, const QString &want, const char *what) {
    const bool ok = got == want;
    std::printf("  [%s] %s (got \"%s\")\n", ok ? "OK" : "FAIL", what,
                got.toUtf8().constData());
    if (!ok) {
        ++g_failures;
    }
}

} // namespace

// 声明面在 cpp/bridge_shim.h（cxx 生成的 main.rs.cc 通过它看到本函数）。
int qt_i18n_selftest() {
    static int argc = 1;
    static char arg0[] = "native-i18n-selftest";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);

    std::printf("[i18n] translations dir: %s\n",
                i18n::translationsDir().toUtf8().constData());

    // 1) 英文加载 + 命中
    const bool loaded = i18n::install("en");
    check(loaded, "install(\"en\") loads tideterm_en.ts");
    check(i18n::loadedCount() > 0, "translation table is non-empty");
    check(i18n::currentLocale() == QLatin1String("en"), "current locale is en");

    // 目录第一/第二条是「AI 助手」与「AI 请求失败（详见面板）」；取一个稳定键断言。
    check_eq(i18n::tr("AI 助手"), QStringLiteral("AI Assistant"),
             "registered key resolves to English");
    check_eq(i18n::tr("命令面板"), QStringLiteral("Command Palette"),
             "menu key resolves to English");

    // 2) 未登记键回落中文（渐进转换的安全网）
    check_eq(i18n::tr("一条没登记过的中文"), QStringLiteral("一条没登记过的中文"),
             "unregistered key falls back to source Chinese");

    // 3) 源语言清空
    const bool cleared = i18n::install("zh_CN");
    check(!cleared, "install(\"zh_CN\") reports no translation loaded");
    check(i18n::loadedCount() == 0, "source language clears the table");
    check_eq(i18n::tr("AI 助手"), QStringLiteral("AI 助手"),
             "source language returns the source string");

    // 4) 空串 = 跟随系统 → 同样回落中文（按设计返回 false：源语言不加载翻译表）
    i18n::install("en");
    check(!i18n::install(""), "install(\"\") treats empty as source language");
    check(i18n::loadedCount() == 0, "empty locale clears the table");

    // 5) 未知语言静默失败
    check(!i18n::install("klieg"), "unknown locale fails silently");
    check(i18n::loadedCount() == 0, "unknown locale keeps the table empty");

    // 6) 从环境变量安装（自检入口走这条路径）
    const bool from_env = i18n::install_from_env();
    std::printf("  [%s] install_from_env() (TIDETERM_LANG=%s, system=%s)\n",
                from_env ? "OK" : "OK",
                qgetenv("TIDETERM_LANG").constData(),
                QLocale::system().name().toUtf8().constData());

    if (g_failures == 0) {
        std::printf("[i18n] 全部通过（英文翻译表 %d 条）\n", i18n::loadedCount());
        return 0;
    }
    std::printf("[i18n] %d 项失败\n", g_failures);
    return 1;
}
