# 架构与技术栈 / Architecture

当前客户端为 Qt Widgets/C++17 视图和 Rust 核心，通过 plain `cxx` 桥交换值类型。生产界面不使用 WebView、Tauri 或 Slint。

```text
Qt Widgets / QPainter
        ↕ cxx（值类型、绘制记录、显式操作）
native Rust 宿主
   ├─ tiderm-core       协议、会话、配置、凭据、AI API、SFTP
   ├─ tiderm-terminal   网格、解析适配、绘制记录、输入、历史和选择
   └─ tiderm-textdoc    行访问契约、文件索引与压缩存储
```

## 目录职责

| 路径 | 职责 |
| --- | --- |
| `native/cpp` | 主窗口、终端视图、会话/文件/设置界面、Qt 自检 |
| `native/src` | Rust 宿主、bridge、Qt 与核心之间的操作和生命周期 |
| `crates/tiderm-core` | 不依赖 UI 的业务、传输与安全模型 |
| `crates/tiderm-terminal` | 与 UI 框架无关的终端引擎和宿主注入边界 |
| `crates/tiderm-textdoc` | RowProvider 契约、日志索引及可选压缩功能 |
| `vendor` | 本地修改的 Alacritty、VTE、portable-pty、libssh2-sys 等 |
| `native/scripts` / `scripts` | 构建、门禁、性能探针与打包工具 |

## 绘制与状态

Rust 维护终端模型与业务状态，Qt 维护焦点、布局、筛选、草稿等视图状态。生产路径从 Rust 生成可见网格的无像素 text spans，经 bridge 交给 QPainter 绘制文字；Rust 不为每个 Qt 窗格长期持有两份全屏 RGBA。窗口后备缓冲、字体缓存和 Qt 平台资源仍然占内存。

密集 ASCII 帧可复用每视图有界字形布局；字体、设备、DPI 等变化使缓存失效。CJK、非 BMP 与 IME 沿用相应文字路径。`tiderm-terminal` 的 `gpu`/wgpu 功能是可选实验，不是默认 Qt 生产绘制或已验收的 GPU 方案。

Windows PowerShell 的窄屏重绘有时不重发输入标记。宿主仅在现有输入租约仍有效、没有未确认的编辑且可见内容符合标准空提示符时恢复锚点；识别最多读取 16 个物理行和 4096 字节，保留后续草稿空格，程序输出不能重开输入租约。

## 协议与任务

`tiderm-core` 使用 Tokio 管理异步工作，本地 PTY 使用 portable-pty，SSH/SFTP 当前仍是 ssh2/libssh2；Windows 使用 OpenSSL 后端，未迁移为 russh/ring。Qt 通过 Rust 发起操作，不绕过协议层直接建立传输。

SSH 主机密钥验证、凭据解锁、认证与连接状态分别处理。普通配置不序列化密码/passphrase；显式记住的凭据进入使用 Argon2 + AES-GCM 的凭据库。不要把这套设计说明当作独立安全审计证明。

文件传输、压缩和查询关注超时、取消、有界队列及来源失效；会话关闭或切换后拒绝迟到结果。可选 AI 功能通过 HTTP API 调用用户配置的服务，不是内置离线模型。

## 历史与文件

每窗格默认 32 MiB 按需历史预算，设置范围 4–512 MiB；预算不是全局进程硬上限，也不包括 Qt、字体、网络、Shell 等开销。历史含热/冷层、压缩、行与重复前缀共享；压缩行为依赖输入，唯一内容可能更慢或更占堆。

重压缩使用共享惰性 worker 和有界排队，较小批次同步处理。历史导出是不可变主屏快照的 UTF-8 流式写入，分页每次最多 64 KiB；磁盘旁路和显式 mmap 不在默认实时滚动路径。具体用户语义见 [Usage](Usage.md)。

来源与派生模块许可证见 [Licenses](Licenses.md)。
