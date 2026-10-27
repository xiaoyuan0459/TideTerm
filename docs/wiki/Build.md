# 构建与运行 / Build

从仓库根目录执行以下命令。各 Cargo workspace 独立维护锁文件；使用 `--locked`，不要把某一个 workspace 的成功当作全部模块通过。

## Windows

依赖：`rust-toolchain.toml` 指定的 Rust 1.99.0、MSVC C++ 工具和 Windows SDK、Qt 6.12.0 MSVC x64 Widgets 套件、PowerShell 7，以及可加载 `IPC::Cmd`、`FindBin` 的 Perl。使用 VS Developer PowerShell，或确保编译器工具链可发现。

```powershell
$env:QT_DIR = 'C:\Qt\6.12.0\msvc2022_64' # 改为本机套件目录
$env:PATH = "$env:QT_DIR\bin;$env:PATH"
. ./native/scripts/prepare-windows-build.ps1
if ($LASTEXITCODE -ne 0) { throw 'Build preparation failed' }
cargo build --manifest-path native/Cargo.toml --bin native --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
& ./native/target/release/native.exe --window
```

`QT_DIR` 下应有 `include/QtWidgets`、`lib`、`bin`。准备脚本设置当前进程的 UTF-8/C++ 环境和 OpenSSL 构建适配器；Windows SSH 使用静态 OpenSSL。适配器按源码哈希缓存，对不匹配的 OpenSSL 构建缓存按 profile 迁移；首次构建仍可能较慢。Qt 仍需自己的 DLL 和平台插件，不能因 OpenSSL 静态链接就把整个程序称为单文件版。

切换 Debug/Release 或运行 C++ 警告门禁时仍使用同一准备脚本，不手动切换全局 `CC_ENABLE_DEBUG_OUTPUT`。修改 native C++ 输入后，当前 cc-rs 路径仍会编译全部注册翻译单元；逐文件输入追踪不等于对象级增量编译。

## Linux / WSL

历史开发构建环境为 Ubuntu 24.04 / WSL2 / WSLg、Qt 6.4.2。以下为同类环境的入口，系统包版本以实际发行版为准：

```bash
sudo apt-get install build-essential pkg-config qt6-base-dev libssl-dev libudev-dev fontconfig fonts-dejavu-core fonts-noto-cjk
cargo build --manifest-path native/Cargo.toml --bin native --locked
./native/target/debug/native --window
```

Linux 通过 `pkg-config` 查找 Qt6Widgets。这里有开发构建记录，尚无正式 Linux 包、Wayland 或真实系统 IME 全量验收。macOS 当前未验证，不提供已支持承诺。

## 配置隔离

```powershell
& ./native/target/release/native.exe --window --config-dir 'C:\TideTerm-Test' --safe-mode
```

`--config-dir` 必须是绝对目录，将该进程的应用配置、主机信任记录和 Qt 布局放在独立目录；参数错误时不回退写默认配置。默认配置位于系统配置目录下 `com.tideterm.app`。不要将个人配置、凭据库、主机记录或日志放入源码仓库。

`--safe-mode` 使用支持的默认 Shell 的无用户配置/不保存历史启动方式；它不改变系统权限，也不构成沙箱。

## 排查与分发

- 找不到 Qt DLL/平台插件：检查 `QT_DIR/bin`、`PATH` 和插件路径是否来自同一套件。
- Perl 缺核心模块：先修复 Perl 环境；不要忽略准备脚本错误后继续构建。
- 开发二进制缺运行时：用匹配 SDK 的 `windeployqt` 部署，并按[许可说明](Licenses.md)核对所有实际模块、字体及库的通知。
- `release-native.yml` 在 master 更新、v* 标签与 PR 时自动生成 Windows 开发 ZIP，也可手动触发。包内提供 Qt 运行库、第三方通知、Rust/Qt SBOM 和构建身份，移除 SDK 路径后运行基础自检，并在另一个未检出源码、未安装 Qt 的 Windows runner 上下载 ZIP 做校验与自检；不自动创建正式 Release。尚无签名/干净机/稳定版的统一验收结论。

验证入口见 [Development](Development.md)。
