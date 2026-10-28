# TideTerm portable-pty patch

Upstream: portable-pty **0.9.0**, published by the WezTerm project, MIT license
(see `LICENSE.md`). Source copied from the Cargo registry package; registry
checksum: `b4a596a2b3d2752d94f51fac2d4a96737b8705dddd311a32b9af47211f08671e`.
Native and tiderm-core both patch crates.io
to this one path package, keeping the same version and upstream API.

2026-10-05: remove the hard-coded `PSEUDOCONSOLE_INHERIT_CURSOR` flag in
`src/win/psuedocon.rs::PsuedoCon::new`. The 0.8.1 flags are restored while the
0.9.0 dependency/API improvements remain. The resize quirk and Win32 input-mode
flags are unchanged. Independent GUI panes start with their own empty viewport;
they do not inherit a parent console's cursor position.

Upstream 0.9.0 always enables cursor inheritance and has no public flag setter.
Windows then requests cursor state asynchronously during ConPTY initialization.
Waiting for the first UI poll to answer that request blocks a finite shell's
startup and breaks output retention before UI construction. See
[Microsoft CreatePseudoConsole documentation](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole).

Regression owner: native
`rust::term_tab_tests::startup_output_is_retained_before_first_poll` (unchanged
five-second deadline and assertions). The registry 0.9.0 build failed this
test with `finite shell did not exit`; patch validation is recorded in the
[Qt remediation report](../../docs/reports/qt-6.12-build-and-obsolescence-audit-2026-10-05.md).
Other checks cover real PTY IO/resize/disconnect, saved PowerShell 7 and Git Bash
options, Qt C4 input/render/output and the full Release suite.

This patch adds no test-only API and changes no Unix code. Reassess the flag on
future upstream upgrades; do not remove the before-first-poll regression guard.
