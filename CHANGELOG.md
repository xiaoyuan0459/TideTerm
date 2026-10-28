# Changelog

## Unreleased

The current client uses Qt Widgets/C++ and Rust. Recent work covers bounded scrollback tools, workspace/session workflows, terminal prompt recovery, Chinese/English UI and Windows build-cache preparation.

There is no stable release yet. See [Usage](docs/wiki/Usage.md) for current behavior and limitations, and [Performance](docs/wiki/Performance.md) for dated measurements. Development build artifacts are not a stability guarantee.

- Automatic Windows packages for master, version tags and pull requests, including checksums, SBOMs and dependency notices.
- Updated TLS, async IO, compression and font dependencies to resolve the audited advisories.
- English/Chinese README and a concise technical Wiki; personal development material stays outside public history.
- SFTP downloads reserve local destinations across sessions until worker completion. Remote file replacements preserve POSIX permissions and abort safely if permission restoration fails.
