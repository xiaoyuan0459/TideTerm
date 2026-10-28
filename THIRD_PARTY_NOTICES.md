# Third-party notices

TideTerm's own code uses [Apache-2.0](LICENSE). Third-party code, fonts and runtime libraries retain their respective notices and licenses. A Cargo dependency graph and a release SBOM describe different artifacts; neither is a substitute for these files.

| Component | Source and license evidence | Local notices |
| --- | --- | --- |
| Hyperpanes-derived portions of tiderm-terminal | [Eyalm321/Hyperpanes](https://github.com/Eyalm321/hyperpanes), MIT, copyright 2026 Eyalm321. Imported from the `heart258/hyperpanes` `tide-mod` fork at `d3f17e6e698cf9834f9a168d1c6af61e8a6510b5`; TideTerm import `76a2dd9`, path migration `b592494` | [engine provenance](crates/tiderm-terminal/NOTICE.md), [original MIT license](crates/tiderm-terminal/LICENSE-MIT). The old core and widget packages are removed; retained derived modules continue to carry MIT |
| alacritty_terminal | Vendored terminal model with local history/parser integration changes | [Apache-2.0](vendor/alacritty_terminal/LICENSE-APACHE) |
| portable-pty | WezTerm component, vendored and locally patched | [license](vendor/portable-pty/LICENSE.md), [patch record](vendor/portable-pty/TIDETERM-PATCH.md) |
| vte | Vendored parser with local changes | [MIT](vendor/vte/LICENSE-MIT), [Apache-2.0](vendor/vte/LICENSE-APACHE) |
| libssh2-sys / libssh2 | Rust wrapper and bundled C library have separate notices | [wrapper MIT](vendor/libssh2-sys/LICENSE-MIT), [wrapper Apache-2.0](vendor/libssh2-sys/LICENSE-APACHE), [libssh2 notices](vendor/libssh2-sys/libssh2/COPYING) |
| JetBrains Mono / Symbols Nerd Font | Embedded font assets, independent of source-code licensing | [JetBrains Mono](crates/tiderm-terminal/assets/fonts/JetBrainsMono-OFL.txt), [Symbols Nerd Font](crates/tiderm-terminal/assets/fonts/SymbolsNerdFont-LICENSE) |
| OpenSSL / openssl-src | Windows SSH statically links the configured OpenSSL build | [notices](native/licenses/NOTICE.txt), [OpenSSL](native/licenses/OpenSSL-LICENSE.txt), [openssl-src](native/licenses/openssl-src-LICENSE-MIT.txt) |
| Qt runtime | The Qt modules and plugins actually shipped must be inventoried with their SDK notices | The SDK is external to the source tree. [Qt 6.12.0 notice texts and provenance](native/licenses/qt/README.md) are retained for qtbase/qtsvg; packaging also copies their actual SDK SBOMs |

The original Hyperpanes license was recovered from the retained Git objects at the exact imported fork commit and independently checked against the [public upstream LICENSE](https://github.com/Eyalm321/hyperpanes/blob/main/LICENSE). The imported core `Cargo.toml` blob was `8118aa2667b396d124c1ea57db4f2a73e135b76c`; its `src/lib.rs` blob was `c70226ae28291be2b78aea698380316e0f5bc94b`, both matching that fork commit. That legacy core was subsequently removed after its two remaining production boundaries were rewritten. TideTerm has substantially changed the retained engine; the upstream product's behavior and validation do not describe TideTerm.

## UI inspiration and AI assistance

[WindTerm](https://github.com/kingToolbox/WindTerm) is a reference for workspace layout, density and interactions. It is not listed as a runtime dependency. TideTerm is independent and does not imply endorsement or feature/performance parity. Reference screenshots and capture scripts are excluded from the current public source tree; removing them from the tip does not remove historical Git objects.

Development uses substantial AI assistance. That does not change third-party attribution or substitute for human review and validation.
