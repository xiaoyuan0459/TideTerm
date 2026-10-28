# TideTerm

[English](README.md) · [简体中文](README.zh-CN.md) · [Documentation](docs/wiki/Home.md)

A native terminal workspace built with **Qt Widgets, C++ and Rust** for people who work across local shells and remote machines.

TideTerm brings SSH, file transfers, split sessions and tools for reading terminal output into one desktop app. It is a personal project under active development; there is no stable release yet.

## Why TideTerm

- **Native desktop UI.** Qt Widgets draws the interface and terminal text; the current app does not embed a browser runtime.
- **A workspace for multiple sessions.** Tabs, independent tab groups, splits, a session tree, quick connections and explicit synchronized input.
- **Tools for long output.** Search, reading mode, rectangular and multiple selections, command blocks, nested folding, text marks and compressed scrollback.
- **Work close to the terminal.** Local/SFTP file browsing and transfers, a command Sender, session logs and workspace restoration.
- **Resource use with explicit budgets.** Scrollback defaults to an on-demand 32 MiB budget per pane, with bounded queues and caches. This is a history budget, not a process memory cap.
- **An inspectable Rust core.** Protocols, credentials and storage live behind a plain `cxx` bridge, separate from the Qt view.

## What works today

Local shells; SSH/SFTP with host-key verification and password/key/interactive authentication; basic Telnet, serial and TCP sessions; Chinese/English UI; light/dark themes; file operations and transfer queues. Optional AI command generation and error explanation use a configured external API and remain experimental.

Workspace restore creates new processes/connections; it does not restore the old running process or terminal output. Explicit tmux attach/detach tools require tmux on the target and do not mirror native pane layouts. See [usage and limits](docs/wiki/Usage.md).

## Performance and memory

These are **historical Windows Release measurements from 2026-10-25**, not promises for every machine or a benchmark of the current tree.

| Scenario | Recorded result | Meaning |
| --- | --- | --- |
| Synthetic high-frequency output | Qt Paint rate 49.988 → 62.472/s; frame-interval p95 21.5164 → 17.3389 ms | About 25% more paint events; single-core CPU rose 27.792% → 32.373% |
| One idle local-shell window | 71.1 MiB private commit; 155.1 MiB working set; **63.6 MiB private working set** | Main process only, 1488×992 physical client area, DPI 119 |
| Earlier dense ASCII/CJK drawing optimization | Window-update p95 5.7527 → 3.8650 ms | A separate matched comparison; cannot be added to the improvement above |

Qt paint events are not display-presented FPS. Memory changes with window size, fonts, history, panes and workload; child shells are excluded. A universal **below-30-MiB** target has not been demonstrated. Conditions, all retained samples and tradeoffs are in [Performance](docs/wiki/Performance.md).

## Automatic Windows packages

The [Windows packaging workflow](https://github.com/xiaoyuan0459/TideTerm/actions/workflows/release-native.yml) runs on `master` updates, `v*` tags and pull requests. Its artifacts include a ZIP, SHA-256 checksums, Rust SBOM and build information. The ZIP contains the Qt runtime, translations and third-party notices, and is checked without the SDK on `PATH`. These remain development builds; inspect the workflow result for the exact commit before downloading.

## Build and run

The main development platform is Windows. Linux/WSL has development-build evidence; macOS and production Linux packages remain unverified.

| Platform | Build prerequisites |
| --- | --- |
| Windows | Rust 1.99.0, MSVC C++ tools + Windows SDK, Qt 6.12.0 MSVC kit, PowerShell 7, Perl with its core modules |
| Linux / WSL | Rust 1.99.0, C++ toolchain, pkg-config, Qt Widgets ≥6.4, OpenSSL development files and fonts |

From the repository root in PowerShell 7:

```powershell
$env:QT_DIR = 'C:\Qt\6.12.0\msvc2022_64' # replace with your Qt kit path
$env:PATH = "$env:QT_DIR\bin;$env:PATH"
. ./native/scripts/prepare-windows-build.ps1
cargo build --manifest-path native/Cargo.toml --bin native --release --locked
& ./native/target/release/native.exe --window
```

Qt's `bin` directory must be on `PATH` when running an undeployed build. Full setup and Linux commands: [Build](docs/wiki/Build.md). For an isolated configuration, add `--config-dir <absolute-directory>`; `--safe-mode` skips user shell startup configuration where supported.

## Technology

| Area | Implementation |
| --- | --- |
| UI and rendering | Qt Widgets/C++17, QPainter text; plain `cxx` bridge |
| Terminal | `tiderm-terminal`, locally maintained Alacritty/VTE code, compressed history |
| Protocols and sessions | Rust `tiderm-core`, Tokio, portable-pty, ssh2/libssh2; OpenSSL for SSH |
| Storage and credentials | `tiderm-textdoc`, JSON configuration, Argon2 + AES-GCM credential vault |

The default Qt renderer uses drawing records/text spans; the optional wgpu experiment is not the production renderer. Details: [Architecture](docs/wiki/Architecture.md).

## Development, inspiration and credits

**TideTerm is developed with substantial AI assistance**, including implementation, debugging and documentation. That does not imply that generated code or all advertised workflows have been independently audited. Human review and reproducible validation remain necessary.

[WindTerm](https://github.com/kingToolbox/WindTerm) inspired the workspace layout, information density and several terminal interactions. TideTerm is an independent project, without affiliation or endorsement, and does not claim feature or performance parity. Some engine modules derive from Hyperpanes; their original MIT notices are retained. See [third-party notices](THIRD_PARTY_NOTICES.md).

Own code is licensed under [Apache-2.0](LICENSE); third-party components retain their licenses. [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Technical documentation](docs/wiki/Home.md).
