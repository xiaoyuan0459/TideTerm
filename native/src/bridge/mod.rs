//! 桥域模块（ADR-014 §Decision 3）：按域拆桥的落位。
//!
//! 每个域一个文件、一个 `#[cxx::bridge]` mod、一份生成头（`src/bridge/<域>.rs.h`）。
//! 各域只声明本域函数；**跨域共享的 POD 仍归主桥**（cxx 为每个桥生成独立类型定义，
//! 同名 struct 在两桥中是两个 C++ 类型——拆域时按域归属切分，不重复声明）。

pub mod ai;
pub mod commands;
pub mod completion;
pub mod context;
pub mod files;
pub mod log;
pub mod marks;
pub mod modem;
pub mod multiline;
pub mod onekey;
pub mod palette;
pub mod pod;
pub mod prefs;
pub mod reading;
pub mod scaffold;
pub mod selection;
pub mod session;
pub mod sftp;
pub mod sync;
pub mod vault;
pub mod workspace;

/// Dispatch the additional terminal self-tests without enlarging the main bridge.
pub fn run_terminal_selftest(args: &[String], png: &str) -> Option<i32> {
    if args.iter().any(|a| a == "--history-selftest") {
        Some(reading::reading_ffi::qt_boot_history(png))
    } else if args.iter().any(|a| a == "--reading-selftest") {
        Some(reading::reading_ffi::qt_boot_reading(png))
    } else if args.iter().any(|a| a == "--commands-selftest") {
        Some(commands::commands_ffi::qt_boot_commands(png))
    } else if args.iter().any(|a| a == "--selection-selftest") {
        Some(selection::selection_ffi::qt_boot_selection(png))
    } else if args.iter().any(|a| a == "--workspace-selftest") {
        Some(workspace::workspace_ffi::qt_boot_workspace(png))
    } else {
        None
    }
}

/// Keep the startup backend policy explicit; an existing platform override wins.
pub fn offscreen_selftest(args: &[String]) -> bool {
    const FLAGS: &[&str] = &[
        "--selftest",
        "--pty-selftest",
        "--g2-selftest",
        "--c3-selftest",
        "--c4-selftest",
        "--c5-selftest",
        "--c6-selftest",
        "--c7-selftest",
        "--c8-selftest",
        "--c9-selftest",
        "--reading-selftest",
        "--history-selftest",
        "--commands-selftest",
        "--selection-selftest",
        "--workspace-selftest",
        "--welcome-selftest",
        "--offscreen",
    ];
    args.iter().any(|arg| FLAGS.contains(&arg.as_str()))
}
