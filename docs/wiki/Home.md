# TideTerm 技术 Wiki

TideTerm 是 Qt Widgets/C++ + Rust 原生终端工作台，目前处于开发阶段。本文档与源码一起版本化，页面可同步到 GitHub Wiki。

| 页面 | 内容 |
| --- | --- |
| [构建与运行](Build.md) | Windows、Linux/WSL 环境、运行参数和常见问题 |
| [架构与技术栈](Architecture.md) | 模块边界、绘制、协议与存储 |
| [使用与限制](Usage.md) | 会话、输出阅读、Sender、文件、日志、恢复 |
| [性能与内存](Performance.md) | 测量口径、历史样本、收益与代价 |
| [开发与验证](Development.md) | 独立 workspace、检查命令和证据边界 |
| [许可与来源](Licenses.md) | 第三方组件、WindTerm 参考与 AI 辅助开发 |

功能入口不代表每个平台都完成验收。主要开发环境为 Windows；Linux/WSL 有开发构建记录，macOS 未验收。构建、受控自检、真实桌面、外部服务和发行包分别验证。
