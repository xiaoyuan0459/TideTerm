# 许可、来源与 AI 开发 / Licenses and provenance

项目自有代码采用 Apache-2.0，第三方代码、字体和运行库保留各自许可。完整逐组件通知与精确导入来源在仓库根目录 [THIRD_PARTY_NOTICES.md](https://github.com/xiaoyuan0459/TideTerm/blob/master/THIRD_PARTY_NOTICES.md)；终端引擎来源在 [终端引擎来源](https://github.com/xiaoyuan0459/TideTerm/blob/master/crates/tiderm-terminal/NOTICE.md)。这些文件及 vendor 中的原许可证是分发材料，不随个人文档清理删除。

## 引擎与库

终端引擎保留 Hyperpanes 派生模块的 MIT 许可。输入编码、宿主注入等边界由 TideTerm 独立实现，但迁移、拆分和包名变更不意味着全部代码从零原创。Alacritty/VTE、portable-pty、libssh2-sys/libssh2、字体及 OpenSSL 各有原始许可和本地修改记录。

Qt SDK 不在源码树内。分发二进制时按实际使用的 Qt 模块/插件、链接方式和 SDK 许可核对材料，同时保留字体、OpenSSL/libssh2、Rust 依赖等通知。打包脚本按 Cargo 实际依赖图收集上游许可文件，缺失时拒绝打包；Qt 原始许可文本另有来源与哈希记录。SBOM 是依赖清单，不能替代许可文件或实际包检查；尚未完成统一的干净机及正式分发验收。

## WindTerm 参考

[WindTerm](https://github.com/kingToolbox/WindTerm) 是工作台布局、信息密度及部分终端交互的参考对象。TideTerm 是独立项目，没有隶属、合作或背书关系，不宣称全部功能或性能对等。本次公开树不包含用于开发对照的第三方窗口截图与采集脚本。

## AI 辅助开发

项目大量使用 AI 辅助实现、调试和文档编写。AI 参与不免除人工审查、来源署名及有效验证，也不构成独立安全审计。产品可选的 AI API 功能与“使用 AI 开发本项目”是两个独立事实。

## 公开树与 Git 历史

公开源码从经过检查的干净快照开始，不导入个人规则、开发索引、工作报告、旧原型和参考截图。原始开发历史与 PR 保留在私有档案和本地备份中。后续开发从公开 master 建分支，不把旧开发分支直接合入公开仓库；本地私有说明由 .gitignore 隔离。
