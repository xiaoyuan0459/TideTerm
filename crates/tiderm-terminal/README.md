# tiderm-terminal

TideTerm 的终端引擎。`native/` 通过值类型和现有 CXX 桥使用它；本库不依赖 Qt，不创建 PTY/网络连接，也不自行访问系统剪贴板。

- Alacritty/VTE 负责标准终端解析与网格，Swash 提供字体度量和栅格化原语。
- TideTerm 的历史、来源行、命令块、选择/阅读、搜索、折叠和标记统一在本库中；Qt 消费 spans，受控探针可使用软件帧。
- 输入编码直接接收 `KeyEvent` 的功能键，无需桥层制造非空文本。剪贴板通过 `host::ClipboardAccess` 显式注入；未注入时不可访问 OS 剪贴板，Qt 宿主只在 GUI 线程提供服务。
- Windows 宿主的文本发布采用同步 Unicode 数据，仍由 Qt GUI 线程提供服务；引擎接口及无 GUI 的窗格隔离保持不变，详见[使用说明](../../docs/wiki/Usage.md)。
- 视口偏移和输出跟随由同一状态所有者维护；历史范围与行号绑定当前屏幕，备用屏不会因滚动读出主屏历史，退出后主屏历史保留。
- 本地清屏和清历史直接修改活动网格，保留未收完整的输出；历史、行缓存和折叠投影一起退休。备用屏操作保留主屏历史/命令位置，清屏可完整归档一屏后恢复原始窗口上限。
- 输出侧 ED3/RIS 由解析器实际执行的 Handler 操作驱动历史退休，压缩历史、待压缩行、缓存与 Pane 状态一起失效；保留有效参数和分片语义，同步更新等待实际执行。备用屏 ED2/ED3 保留主屏文档，主屏 ED2 完整归档一屏；详见[架构说明](../../docs/wiki/Architecture.md)。
- 实时网格和压缩历史共用解析后的单元格样式，保留反色、暗色、既有属性与彩色空行背景；历史继续保存当时解析的 RGBA。
- 解析器事件由每个网格独立的收件箱交付；标题和响铃到达即合并，标题栈恢复到未设置状态时显式通知宿主，回复字节和剪贴板操作分别按序保留。
- 提示符草稿可按实际光标读取，保留末尾空格与软换行中的组合/宽字符；终端联想的会话、回显和接受状态由 Native 持有，Qt 只展示列表。
- 可选 `gpu` 路径保留用于后续验证，默认生产路径为 Qt spans；不能以默认测试通过推导 GPU、Release 或外部 TUI 全面兼容。

本库整合已有 TideTerm 实现，并保留部分来自 Hyperpanes 的派生模块。输入编码、宿主服务边界、视口状态控制及事件收件箱由 TideTerm 独立实现，单元格样式适配由统一模型维护；其他模块的迁移和拆分不称为从零重写。来源、MIT 许可和字体许可见 [NOTICE](NOTICE.md)。职责与行为边界见[架构](../../docs/wiki/Architecture.md)与[使用说明](../../docs/wiki/Usage.md)。

```powershell
cargo fmt --manifest-path crates/tiderm-terminal/Cargo.toml -- --check
cargo clippy --manifest-path crates/tiderm-terminal/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/tiderm-terminal/Cargo.toml
```
