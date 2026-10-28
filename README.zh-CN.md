# TideTerm

[English](README.md) · [简体中文](README.zh-CN.md) · [技术文档](docs/wiki/Home.md)

面向本地 Shell 与远程服务器工作的原生终端工作台，使用 **Qt Widgets / C++ + Rust** 开发。

将 SSH、文件传输、多会话分屏和终端输出阅读工具放在一个桌面应用里。这是持续开发中的个人项目，目前尚无稳定发行版。

## 项目优势

- **原生桌面界面**：Qt Widgets 绘制界面与终端文字，当前客户端无需嵌入浏览器运行时。
- **多会话工作台**：标签、独立标签组、分屏、会话树、快速连接，以及需要显式开启的同步输入。
- **长输出更易阅读**：搜索、阅读模式、矩形/多重选择、命令块、嵌套折叠、文本标记与压缩回滚历史。
- **终端旁完成日常工作**：本地/SFTP 文件浏览和传输、命令发送栏、持续日志与工作区恢复。
- **资源开销有预算**：默认每窗格 32 MiB 按需回滚预算，队列和缓存有界。历史预算不等于进程总内存上限。
- **Rust 核心便于检查与演进**：协议、凭据和存储通过 plain `cxx` 桥与 Qt 视图分离。

## 当前功能

本地 Shell；带主机密钥验证及密码/密钥/交互认证的 SSH/SFTP；基础 Telnet、串口、TCP；中英文界面、明暗主题、文件操作及传输队列。可选的 AI 命令生成和错误解释连接用户配置的外部 API，目前仍属实验功能。

工作区恢复会新建进程与连接，不能恢复原运行进程或输出。tmux 附加/分离需目标安装 tmux，目前不映射原生分屏布局。详见[使用与边界](docs/wiki/Usage.md)。

## 性能与内存

以下是 **2026-10-25 的 Windows Release 历史样本**，不代表所有机器，也不是本次整理后源码的重新测量。

| 场景 | 已记录结果 | 口径 |
| --- | --- | --- |
| 合成高频输出 | Qt Paint 49.988 → 62.472 次/秒；帧间隔 p95 21.5164 → 17.3389 ms | 绘制事件率约升 25%，单核 CPU 同时从 27.792% 升至 32.373% |
| 一个空闲本地 Shell 完整窗口 | 私有提交 71.1 MiB；工作集 155.1 MiB；**私有工作集 63.6 MiB** | 仅主进程，物理客户区 1488×992，DPI 119 |
| 更早的一组密集 ASCII/CJK 绘制优化 | 窗口更新 p95 5.7527 → 3.8650 ms | 独立配对样本，不能与上一项叠加计算收益 |

Qt 绘制事件不等于屏幕实际呈现 FPS。内存随窗口大小、字体、历史、窗格和负载变化，以上不含子 Shell；尚未证明通用的 **低于 30 MiB** 目标。环境、完整保留样本和取舍见[性能文档](docs/wiki/Performance.md)。

## 自动打包

[Windows 打包工作流](https://github.com/xiaoyuan0459/TideTerm/actions/workflows/release-native.yml) 在 `master` 更新、`v*` 标签和 Pull Request 时自动运行，产物包括 ZIP、SHA-256 校验和、Rust SBOM 与构建信息。ZIP 包含 Qt 运行库、翻译和第三方声明，并在移除 SDK 路径后做基础自检。这些仍是开发构建，下载前请核对对应提交的工作流结果。

## 构建与运行

Windows 是当前主要开发平台。Linux/WSL 有开发构建记录，macOS 与 Linux 正式分发包尚未验证。

| 平台 | 构建依赖 |
| --- | --- |
| Windows | Rust 1.99.0、MSVC C++ 工具与 Windows SDK、Qt 6.12.0 MSVC 套件、PowerShell 7、含核心模块的 Perl |
| Linux / WSL | Rust 1.99.0、C++ 工具链、pkg-config、Qt Widgets ≥6.4、OpenSSL 开发库与字体 |

在仓库根目录使用 PowerShell 7：

```powershell
$env:QT_DIR = 'C:\Qt\6.12.0\msvc2022_64' # 按实际安装目录修改
$env:PATH = "$env:QT_DIR\bin;$env:PATH"
. ./native/scripts/prepare-windows-build.ps1
cargo build --manifest-path native/Cargo.toml --bin native --release --locked
& ./native/target/release/native.exe --window
```

未部署构建运行时还需把 Qt `bin` 加到 `PATH`。完整环境准备与 Linux 命令见[构建指南](docs/wiki/Build.md)。使用 `--config-dir <绝对目录>` 隔离配置；`--safe-mode` 可在支持的 Shell 上跳过用户启动配置。

## 当前技术栈

| 层次 | 实现 |
| --- | --- |
| 界面与绘制 | Qt Widgets / C++17、QPainter 文字、plain `cxx` 桥 |
| 终端 | `tiderm-terminal`、本地维护的 Alacritty/VTE、压缩历史 |
| 协议与会话 | Rust `tiderm-core`、Tokio、portable-pty、ssh2/libssh2；SSH 使用 OpenSSL |
| 存储与凭据 | `tiderm-textdoc`、JSON 配置、Argon2 + AES-GCM 凭据库 |

Qt 默认使用绘制记录/text spans；可选 wgpu 实验路径尚非生产渲染器。详见[架构说明](docs/wiki/Architecture.md)。

## AI 开发与参考来源

**本项目大量使用 AI 辅助开发**，包括实现、排错和文档整理。AI 参与不代表生成代码及全部功能已经独立审计，仍需人工审查与可复现验证。

工作台布局、信息密度和部分终端交互参考了 [WindTerm](https://github.com/kingToolbox/WindTerm)。TideTerm 为独立项目，与 WindTerm 无隶属或背书关系，也不宣称功能或性能完全对等。部分引擎模块派生自 Hyperpanes，保留原 MIT 许可和来源记录，见[第三方声明](THIRD_PARTY_NOTICES.md)。

自有代码采用 [Apache-2.0](LICENSE)，第三方组件保留各自许可。[参与贡献](CONTRIBUTING.md) · [安全报告](SECURITY.md) · [技术文档](docs/wiki/Home.md)。
