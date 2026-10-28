# logstore-probe

Independent developer CLI for generating synthetic logs and measuring `tiderm-textdoc` indexing, paging and compression. It is not the interactive terminal renderer or a benchmark of SSH/network throughput.

From the repository root:

```powershell
cargo run --manifest-path tools/logstore-probe/Cargo.toml --release --locked -- gen-corpus tools/logstore-probe/target/corpus/app-1g.log 1024
cargo run --manifest-path tools/logstore-probe/Cargo.toml --release --locked -- bench tools/logstore-probe/target/corpus/app-1g.log
cargo test --manifest-path tools/logstore-probe/Cargo.toml --locked
```

The generation example creates about 1 GiB of synthetic data; select a smaller size where appropriate. Keep generated data under `target/`. CLI production dependencies use the log library; the terminal dependency is used by the contract tests.

Record disk, build, cache state, data size, memory metric and all samples when reporting results. See [Development](../../docs/wiki/Development.md) and [Performance](../../docs/wiki/Performance.md).
