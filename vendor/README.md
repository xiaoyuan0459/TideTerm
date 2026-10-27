# 定制第三方依赖

| 目录 | 用途 |
| --- | --- |
| `alacritty_terminal/` | 终端网格模型（`Term`/`Grid`/`Storage`）。补丁：原始历史窗按调用方的 `max_scroll_limit` 决定备用行数，不再无条件预分配 1000 行（见 `src/grid/storage.rs` 的 `cache_limit`）。 |
| `libssh2-sys/` | ssh2 的 C 绑定与 vendored libssh2，保留项目通道窗口补丁。 |
| `portable-pty/` | 本地 PTY 后端。 |
| `vte/` | ANSI 解析器。补丁：同步更新缓冲（DECSET 2026）改懒增长，不再每处理器预分配 2 MiB。 |

`[patch.crates-io]` 在两个工作区各自声明：[native/Cargo.toml](../native/Cargo.toml)（Qt 壳）与
[crates/tiderm-terminal/Cargo.toml](../crates/tiderm-terminal/Cargo.toml)（终端引擎）。

`alacritty_terminal` 只拷 `src/` 与许可证/README（上游 `tests/` 是 46 MB 的参考帧，不参与构建），
并在拷贝的 `Cargo.toml` 里去掉 `[[test]]` 与 `[dev-dependencies]`；其余保持上游原样。
打补丁的每一处都在源码里用 `TideTerm patch:` 注释标出，便于上游修复后回收。

保持各组件的上游目录、测试、许可证与构建文件；不能把看似没有直接调用的上游文件当作项目临时文件删除。
