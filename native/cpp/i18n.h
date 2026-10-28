// Qt 壳多语言基础设施（2026-09-22 增补，替代已剔除 Slint 壳的 gettext/.po）。
//
// 设计约束：
// 1. **源语言即中文**：代码里的文案就是中文，`tr()` 对中文原样返回；只有
//    `i18n/*.ts` 里登记了译文的语言才会被替换。未登记条目自动回落中文，
//    因此可以逐面渐进转换，不要求一次性覆盖全部文案。
// 2. **不引入构建期翻译工具**：lrelease/lupdate 只装在部分开发机的 Qt bin
//    里（`run-selftests.ps1` 硬编码路径那类问题），CI 与门禁不依赖它们。
//    `.ts` 用 Qt 标准 XML 格式书写，运行时由 QXmlStreamReader 解析——
//    将来若要切回 lrelease 编译 .qm，只需替换本文件的加载实现。
// 3. **查找键 = 源串本身**（不是符号 id）：改写存量文案时把
//    `QStringLiteral("保存")` 换成 `i18n::tr("保存")` 即可，无需维护 id 表。
//
// 运行时数据：`<exe 目录>/i18n/tideterm_<lang>.ts`（如 `tideterm_en.ts`）。

#pragma once

#include <QHash>
#include <QString>

namespace i18n {

/// 解析并安装指定语言的翻译表（幂等；重复调用会先清空旧表）。
/// `locale` 为空或 "zh_CN"（源语言）时清空翻译表——界面回到中文。
/// 文件缺失或解析失败时静默保持当前状态（调用方据此决定是否提示）。
/// 返回是否成功加载了非空翻译表。
bool install(const QString &locale);

/// 按运行环境决定语言并安装：`TIDETERM_LANG` 环境变量 > `QLocale::system()`。
/// 自检入口用这個（不必接偏好桥）；生产入口仍以 `prefs.locale` 优先，
/// 仅在偏好为空（跟随系统）时回落到本函数。
bool install_from_env();

/// 翻译一条 UI 文案：命中翻译表返回译文，否则原样返回源串。
/// 未调用 install()（或源语言）时等价于返回 `source`。
QString tr(const char *source);
QString tr(const QString &source);

/// 当前生效语言（空串 = 中文源语言）。
const QString &currentLocale();

/// 已加载的翻译条数（0 = 未加载或源语言）。
int loadedCount();

/// 翻译表所在目录（可执行文件目录下的 i18n/）。
QString translationsDir();

} // namespace i18n
