# 开发与验证 / Development

项目有多个独立 Cargo workspace，按变更范围运行相关命令。下列是可用检查入口，列出命令不代表本版本已经全部执行。

## Rust

在完成 [Build](Build.md) 的环境准备后执行，每条命令分别检查退出码：

```powershell
cargo fmt --manifest-path native/Cargo.toml --package native -- --check
cargo clippy --manifest-path native/Cargo.toml --locked --all-targets --no-deps -- -D warnings
cargo test --manifest-path native/Cargo.toml --locked --bin native
cargo fmt --manifest-path crates/tiderm-core/Cargo.toml --package tiderm-core -- --check
cargo clippy --manifest-path crates/tiderm-core/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path crates/tiderm-core/Cargo.toml --locked
cargo fmt --manifest-path crates/tiderm-terminal/Cargo.toml --package tiderm-terminal -- --check
cargo test --manifest-path crates/tiderm-terminal/Cargo.toml --locked
cargo test --manifest-path crates/tiderm-textdoc/Cargo.toml --locked
```

不要使用跨 vendor 的 `cargo fmt --all` 意外修改上游代码。新增或修改行为的测试应保护用户行为、公共契约或生命周期，避免只检查源码字面和实现细节。真实网络测试默认隔离/忽略，显式配置测试目标后单独运行。

## Qt/C++ 与构建准备

```powershell
pwsh -NoProfile -File native/scripts/check-cpp-warnings.ps1 -Configuration debug
pwsh -NoProfile -File native/scripts/check-i18n-coverage.ps1
pwsh -NoProfile -File native/scripts/test-build-preparation.ps1
$sourceRoot = (Get-Location).Path
Push-Location $env:TEMP # 先将 TEMP/TMP 配置为本次测试独占的短目录
try {
    New-Item -ItemType Directory -Force -Path native/target | Out-Null
    pwsh -NoProfile -File "$sourceRoot/native/scripts/run-selftests.ps1" -LogDirectory "$sourceRoot/native/target/selftests"
} finally { Pop-Location }
```

C++ 门禁默认串行采证（`-BuildJobs 1`），避免并行日志截断诊断路径；强制重编并分类项目、已知 Qt、其他诊断。不能只看 Cargo exit 或把已知 Qt 诊断称为全仓零警告。Qt 批跑使用 offscreen，自检开关清单位于 `native/scripts/selftest-flags.txt`。运行前先构建对应 configuration。构建准备回归在临时 Cargo fixture 中检查 Debug/Release 缓存迁移和无关产物保留。

Windows 测试使用短、隔离的 TEMP/TMP；临时路径也会进入 Git 引用路径和 Shell 提示符，过长会触发平台路径上限并改变窄屏场景。CI 显式使用 runner.temp，并从该目录运行 Qt 自检；先创建 native/target 作为合成诊断图片的输出目录。

GUI 验证和性能批次串行运行，使用独立配置、临时目录和唯一诊断目录；性能采样脚本持有 GUI 文件锁。不要把测试写到个人会话、信任文件或凭据库。Windows PTY 中断测试应由原生 PowerShell 宿主运行，MSYS 宿主的信号语义不同。

## 仓库、性能与 CI

```bash
bash scripts/check-file-size.sh
bash scripts/verify.sh
```

手写 Rust/C++ 文件一般上限为 500 行，现有豁免以脚本为准。`verify.sh` 是三阶段快捷入口，不覆盖全部 CI、Qt 桌面、网络和发布检查。保持原有门槛，不通过删测试或放宽阈值制造通过。

性能使用同平台/构建/窗口/DPI/负载，预热一次、三次取中位数并保留全部样本。窗口性能入口为 `native/scripts/measure-window-performance.ps1`，引擎基准和进程内存探针有独立口径；数据及历史条件见 [Performance](Performance.md)。

`.github/workflows/ci.yml` 负责 Rust、终端消费冒烟、Windows Qt、依赖审计和仓库检查；`release-native.yml` 在 master 更新、版本标签与 PR 时自动打包，也可手动触发，`memory-baseline.yml` 提供独立采样。确认最终提交的 CI 结果，不把旧结果套到新提交。

源码、受控/offscreen、真实桌面、外部服务、Release 性能、干净机和远程 CI 分开报告。提交前检查差异及敏感信息，运行 `git diff --check`，并请求人工审查。
