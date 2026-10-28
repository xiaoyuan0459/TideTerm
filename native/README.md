# TideTerm native client

The production desktop client uses Qt Widgets/C++17 and Rust through a plain `cxx` bridge. The Cargo package and development executable are named `native`; the application is TideTerm.

- [Build and run](../docs/wiki/Build.md)
- [Architecture](../docs/wiki/Architecture.md)
- [Usage and limitations](../docs/wiki/Usage.md)
- [Validation commands](../docs/wiki/Development.md)
- [Performance and memory](../docs/wiki/Performance.md)

Run `cargo build --manifest-path native/Cargo.toml --bin native --locked` after preparing the Qt/compiler environment. See the build guide for the required Windows preparation script and Qt runtime paths.
