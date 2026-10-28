# tiderm-core

TideTerm 自有共享公共库，负责协议、会话、配置、安全、AI 与 SFTP，不依赖 Tauri、Slint 或 Qt。Qt 客户端通过 Rust 层使用它。

从仓库根目录执行：

```powershell
cargo fmt --manifest-path crates/tiderm-core/Cargo.toml -- --check
cargo clippy --manifest-path crates/tiderm-core/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/tiderm-core/Cargo.toml
```

真实 SSH/SFTP 用例默认忽略，运行环境和范围见 [验证指南](../../docs/wiki/Development.md)。目录迁移不改变应用配置、会话文件或 vault 的格式与位置。
