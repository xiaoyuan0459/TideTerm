// native 构建：cxx 1.0（不用 cxx-qt，ADR-012 §Decision 1）+ 手工 Qt 链接。
//
// 编译 build_sources.rs 显式列举的全部 C++ TU（新增源文件必须登记，否则链接期才报缺符号）。
//
// Windows 使用 QT_DIR/MSVC；Linux 使用 Qt6Widgets 的 pkg-config 配置。

mod build_sources;
use std::path::Path;
use std::{env, path::PathBuf};

/// Qt 模块名（链接顺序：越上层越靠前，Qt 要求被依赖者排后）
const QT_MODULES: &[&str] = &["Qt6Widgets", "Qt6Gui", "Qt6Core"];

/// 兜底拼 MSVC INCLUDE（VC + Windows SDK ucrt/um/shared）。
///
/// 背景（2026-09-14 实测坑）：本机 cc-rs 1.2.63 / find-msvc-tools 对 VS18
/// BuildTools 的探测返回**不带 env 的 cl 路径** → cl 收到空 INCLUDE → `C1083:
/// cstdint`/`C1034: 不包括路径集`（诊断打 stdout，cc-rs 只回传 stderr——表面
/// 现象是 exit 2 无诊断）。历史上构建能过全是 cc-rs .o 缓存命中（cxx 依赖同
/// 理），一旦 clean 全量重编即暴露。此处进程级 set INCLUDE：cl 是子进程直接
/// 继承；MSVC_INCLUDE 环境变量可整体覆盖（CI/异机路径）。
fn ensure_msvc_include() {
    if env::var("INCLUDE").map(|v| !v.is_empty()).unwrap_or(false) {
        return; // 已有（开发者命令提示符 / vcvars 包装）
    }
    if let Ok(custom) = env::var("MSVC_INCLUDE") {
        // SAFETY: build-script startup is single-threaded, before cxx/cc workers.
        unsafe { env::set_var("INCLUDE", custom) };
        return;
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    let vc_tools = env::var_os("VCToolsInstallDir")
        .map(PathBuf::from)
        .or_else(|| {
            let vswhere = PathBuf::from(
                "C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe",
            );
            let output = std::process::Command::new(vswhere)
                .args([
                    "-latest",
                    "-products",
                    "*",
                    "-requires",
                    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                    "-property",
                    "installationPath",
                ])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let install = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            let root = install.join("VC/Tools/MSVC");
            let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
                .ok()?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_dir())
                .collect();
            versions.sort();
            versions.pop()
        });
    let vc_include = vc_tools
        .map(|tools| tools.join("include"))
        .filter(|path| path.is_dir())
        .expect("找不到 MSVC include：请用 VS Developer PowerShell，或设置 MSVC_INCLUDE");
    dirs.push(vc_include);
    // Windows SDK（取最高版本）。
    let sdk_includes = ["ucrt", "um", "shared", "winrt", "cppwinrt"];
    let sdk_root = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Program Files (x86)/Windows Kits/10"))
        .join("Include");
    if let Some(best) = std::fs::read_dir(&sdk_root).ok().and_then(|rd| {
        let mut vs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        vs.sort();
        vs.pop()
    }) {
        for sub in sdk_includes {
            let d = best.join(sub);
            if d.is_dir() {
                dirs.push(d);
            }
        }
    }
    let joined = std::env::join_paths(&dirs).expect("join INCLUDE");
    // SAFETY: build-script startup is single-threaded, before cxx/cc workers.
    unsafe { env::set_var("INCLUDE", &joined) };
    println!("INCLUDE 兜底（{} 目录）", dirs.len());
}

/// 递归登记 `dir` 下所有指定扩展名的文件为 `cargo:rerun-if-changed`。
///
/// 用途：替代 `cargo:rerun-if-changed=cpp`（整目录），避免非构建输入触发全组重编。
/// 递归登记当前源码与头文件，排除目录内非构建输入；新增 TU 仍须登记到 build_sources.rs。
/// 路径统一成正斜杠（cargo 在 Windows 上按登记字符串比对；`cpp\main.cpp` 与
/// `cpp/main.cpp` 不是同一个键）。
fn register_tree(dir: &std::path::Path, extensions: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            register_tree(&path, extensions);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
            && let Some(rel) = path.to_str()
        {
            println!("cargo:rerun-if-changed={}", rel.replace('\\', "/"));
        }
    }
}

/// cxx-build 1.0.202 在本机把内嵌的 `rust/cxx.h` 写成 0 字节（独立最小工程可复现，
/// 与仓库代码无关）：生成的 C++ 每个 TU 都会在 `rust::Str` 处报「不是类或命名空间」。
/// 这里在 cc 编译前用 cxx crate 自带的同一份头补回；头已正常或找不到来源时保持原状，
/// 让编译如实报错，不掩盖别的问题。
fn restore_cxx_header() {
    let Some(out) = env::var_os("OUT_DIR") else {
        return;
    };
    let header = PathBuf::from(out).join("cxxbridge/include/rust/cxx.h");
    if std::fs::metadata(&header)
        .map(|m| m.len() > 0)
        .unwrap_or(true)
    {
        return;
    }
    let Some(cargo_home) = env::var_os("CARGO_HOME").map(PathBuf::from) else {
        return;
    };
    let Ok(sources) = std::fs::read_dir(cargo_home.join("registry/src")) else {
        return;
    };
    for source in sources.flatten() {
        let Ok(crates) = std::fs::read_dir(source.path()) else {
            continue;
        };
        for entry in crates.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("cxx-1") {
                continue; // 只要 cxx 本体；cxx-build 的头在 src/bridge/include 下。
            }
            let candidate = entry.path().join("include/cxx.h");
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            if !bytes.is_empty() && std::fs::write(&header, bytes).is_ok() {
                println!("rust/cxx.h 由 {} 恢复", candidate.display());
                return;
            }
        }
    }
}

fn main() {
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows {
        println!("cargo:rustc-link-lib=shell32");
    }
    let qt = if windows {
        ensure_msvc_include();
        let qt = PathBuf::from(env::var("QT_DIR").expect("Windows 构建前请设置 QT_DIR"));
        assert!(
            qt.join("include/QtWidgets").is_dir(),
            "QT_DIR 缺少 QtWidgets headers"
        );
        Some(qt)
    } else {
        None
    };

    // 多桥（ADR-014 §Decision 3）：主桥内联 main.rs + 各域桥在 src/bridge/，
    // 各域各生成头（native/src/main.rs.h、native/src/bridge/<域>.rs.h）。
    let mut build = cxx_build::bridges([
        "src/main.rs",
        "src/bridge/scaffold.rs",
        "src/bridge/pod.rs",
        "src/bridge/session.rs",
        "src/bridge/sftp.rs",
        "src/bridge/ai.rs",
        "src/bridge/prefs.rs",
        "src/bridge/log.rs",
        "src/bridge/onekey.rs",
        "src/bridge/vault.rs",
        "src/bridge/context.rs",
        "src/bridge/commands.rs",
        "src/bridge/marks.rs",
        "src/bridge/palette.rs",
        "src/bridge/multiline.rs",
        "src/bridge/completion.rs",
        "src/bridge/files.rs",
        "src/bridge/modem.rs",
        "src/bridge/reading.rs",
        "src/bridge/selection.rs",
        "src/bridge/workspace.rs",
        "src/bridge/sync.rs",
    ]);
    restore_cxx_header();
    for path in build_sources::CPP_FILES {
        build.file(path);
    }
    build
        // Keep invocation evidence local: global cc diagnostics invalidate dependency caches.
        .cargo_debug(true)
        // C++ 侧 include 按 crate 根解析（cpp/bridge_shim.h 位于 cpp/）
        .include(".")
        // Qt 模块导出宏：不定义会导致 Q_WIDGETS_EXPORT 走 import 分支
        .define("QT_WIDGETS_LIB", None)
        .define("QT_GUI_LIB", None)
        .define("QT_CORE_LIB", None)
        .std("c++17");

    // ── MSVC 三旗标（实测坑，缺一不可；cxx-qt-build 代劳时看不见）──
    // 坑 1：Qt 的 qcompilerdetection.h 要求真 C++17 —— MSVC 默认 __cplusplus 停在
    //       199711L，必须 /Zc:__cplusplus。
    // 坑 1b：当前 Qt SDK 的 qcompilerdetection.h 要求 /permissive-（漏掉时 QString
    //       模板炸出 type_traits 递归别名错误串，极具误导性）。
    // 坑 2（ADR-011 约束 8）：cc-rs 默认不加 /utf-8，中文注释被按 GBK 解码，
    //       报出"变量未声明"这类误导性错误 —— 对中文注释库是必踩项。
    if let Some(qt) = &qt {
        let include = qt.join("include");
        build.include(&include);
        for module in ["QtCore", "QtGui", "QtWidgets"] {
            build.include(include.join(module));
        }
        for flag in ["/Zc:__cplusplus", "/permissive-", "/utf-8", "/W4", "/EHsc"] {
            build.flag_if_supported(flag);
        }
    } else {
        let qt = pkg_config::Config::new()
            .atleast_version("6.4")
            .probe("Qt6Widgets")
            .expect("Linux 构建需要 pkg-config 和 Qt 6.4+ Widgets 开发包");
        for include in qt.include_paths {
            build.include(include);
        }
        build.flag("-fPIC");
    }

    build.compile("native_cpp");

    if let Some(qt) = qt {
        println!(
            "cargo:rustc-link-search=native={}",
            qt.join("lib").display()
        );
        for m in QT_MODULES {
            println!("cargo:rustc-link-lib=dylib={m}");
        }
    }

    // Track every C++ translation unit and header, including newly split files.
    //
    // Only source/header changes should rerun this script; unrelated files in cpp/ do not.
    // cc-rs recompiles every registered TU when the script runs; per-file tracking does
    // not provide an object cache. New source files must also enter build_sources.rs.
    register_tree(Path::new("cpp"), &["cpp", "h"]);
    register_tree(Path::new("src/bridge"), &["rs"]);
    println!("cargo:rerun-if-changed=cpp/main.cpp");
    println!("cargo:rerun-if-changed=cpp/main_window.cpp");
    println!("cargo:rerun-if-changed=cpp/main_window.h");
    println!("cargo:rerun-if-changed=cpp/bridge_spike.h");
    println!("cargo:rerun-if-changed=src/bridge/scaffold.rs");
    println!("cargo:rerun-if-changed=src/bridge/pod.rs");
    println!("cargo:rerun-if-changed=src/bridge/session.rs");
    println!("cargo:rerun-if-changed=src/bridge/log.rs");
    println!("cargo:rerun-if-changed=src/bridge/onekey.rs");
    println!("cargo:rerun-if-changed=src/bridge/vault.rs");
    println!("cargo:rerun-if-changed=cpp/chrome/app_font.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/app_icon.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/app_icon.h");
    println!("cargo:rerun-if-changed=cpp/chrome/app_font.h");
    println!("cargo:rerun-if-changed=cpp/chrome/docks.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/docks.h");
    println!("cargo:rerun-if-changed=cpp/chrome/live_session_labels.h");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_bar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_bar.h");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_actions.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_actions.h");
    println!("cargo:rerun-if-changed=cpp/chrome/onekey_dialog.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/onekey_dialog.h");
    println!("cargo:rerun-if-changed=cpp/chrome/welcome.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/welcome.h");
    println!("cargo:rerun-if-changed=cpp/chrome/pod_tree_model.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/pod_tree_model.h");
    println!("cargo:rerun-if-changed=cpp/chrome/prompts.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/prompts.h");
    println!("cargo:rerun-if-changed=cpp/chrome/quick_connect.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/quick_connect.h");
    println!("cargo:rerun-if-changed=cpp/chrome/send_bar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/send_bar.h");
    println!("cargo:rerun-if-changed=cpp/chrome/session_banner.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/session_banner.h");
    println!("cargo:rerun-if-changed=cpp/chrome/favorite_toolbar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/favorite_toolbar.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_chrome.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_chrome.h");
    println!("cargo:rerun-if-changed=cpp/chrome/session_form.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/session_form.h");
    println!("cargo:rerun-if-changed=cpp/chrome/prefs_dialog.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/prefs_dialog.h");
    println!("cargo:rerun-if-changed=cpp/chrome/status_bar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/status_bar.h");
    println!("cargo:rerun-if-changed=cpp/terminal/terminal_view.cpp");
    println!("cargo:rerun-if-changed=cpp/terminal/terminal_pointer.cpp");
    println!("cargo:rerun-if-changed=cpp/terminal/terminal_view.h");
    println!("cargo:rerun-if-changed=cpp/terminal/span_paint.h");
    println!("cargo:rerun-if-changed=cpp/terminal/selection_press.h");
    println!("cargo:rerun-if-changed=cpp/bridge_shim.h");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_view.cpp");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_rows.cpp");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_rows.h");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_chrome.cpp");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_transfer.cpp");
    println!("cargo:rerun-if-changed=cpp/sftp/sftp_view.h");
    println!("cargo:rerun-if-changed=cpp/main_window_layout.cpp");
    println!("cargo:rerun-if-changed=cpp/main_window_send_panel.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/ui_metrics.h");
    println!("cargo:rerun-if-changed=cpp/chrome/ui_theme.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/ui_theme.h");
    println!("cargo:rerun-if-changed=cpp/chrome/tree_delegate.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tree_menu.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tree_menu.h");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_close.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/split_ratio.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/split_ratio.h");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_splits.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_splits.h");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_close.h");
    println!("cargo:rerun-if-changed=cpp/chrome/tree_delegate.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_metrics.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_buttons.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_buttons.h");
    // 2026-09-25 WT 对齐批次新增 TU（漏登记 = 改这两个文件时 cargo 不重编 C++）
    println!("cargo:rerun-if-changed=cpp/chrome/panel_header.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_header.h");
    println!("cargo:rerun-if-changed=cpp/chrome/icon_cache.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/icon_cache.h");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_style.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/menu_style.h");
    println!("cargo:rerun-if-changed=cpp/chrome/term_menu.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/term_menu.h");
    // 2026-09-25 快捷栏批次（标签条右端簇）
    println!("cargo:rerun-if-changed=cpp/chrome/tab_quick_bar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_quick_bar.h");
    println!("cargo:rerun-if-changed=cpp/main_window_groups.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_workspace.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_workspace_drag.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/command_history_view.cpp");
    println!("cargo:rerun-if-changed=src/bridge/commands.rs");
    println!("cargo:rerun-if-changed=src/bridge/marks.rs");
    println!("cargo:rerun-if-changed=src/bridge/palette.rs");
    println!("cargo:rerun-if-changed=src/bridge/multiline.rs");
    println!("cargo:rerun-if-changed=src/bridge/completion.rs");
    println!("cargo:rerun-if-changed=cpp/chrome/send_completion.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/send_completion.h");
    println!("cargo:rerun-if-changed=cpp/chrome/send_quickbar.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/command_template_dialog.cpp");
    println!("cargo:rerun-if-changed=assets/completion-rules.json");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_completion.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_completion.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_command_templates.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_command_templates.h");
    println!("cargo:rerun-if-changed=cpp/chrome/user_commands_dialog.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/multiline_dialog.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/multiline_dialog.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_multiline.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_multiline.h");
    println!("cargo:rerun-if-changed=build_sources.rs");
    println!("cargo:rerun-if-changed=cpp/chrome/command_palette.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/command_palette.h");
    println!("cargo:rerun-if-changed=cpp/chrome/palette_model.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/palette_model.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_palette.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_palette.h");

    println!("cargo:rerun-if-changed=cpp/chrome/text_marks.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/text_marks.h");
    println!("cargo:rerun-if-changed=src/bridge/selection.rs");
    println!("cargo:rerun-if-changed=cpp/chrome/command_outline.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/command_outline.h");
    println!("cargo:rerun-if-changed=cpp/chrome/fold_regions.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/fold_regions.h");
    println!("cargo:rerun-if-changed=cpp/chrome/terminal_preview.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/terminal_preview.h");
    println!("cargo:rerun-if-changed=cpp/chrome/hover_preview.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/hover_preview.h");
    println!("cargo:rerun-if-changed=cpp/terminal/terminal_history.cpp");
    println!("cargo:rerun-if-changed=cpp/terminal/terminal_history.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_commands.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_history.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_history.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_folds.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_folds.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_marks.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_marks.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_selection.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/docks_log.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/docks_auxiliary.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_controls.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_drop.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_drop.h");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_drag.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_drag.h");
    println!("cargo:rerun-if-changed=cpp/chrome/tab_workspace.h");
    println!("cargo:rerun-if-changed=cpp/chrome/command_history_view.h");
    println!("cargo:rerun-if-changed=cpp/chrome/panel_controls.h");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_groups.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_panel_drag.cpp");
    println!("cargo:rerun-if-changed=cpp/chrome/selftest_groups.h");
    println!("cargo:rerun-if-env-changed=QT_DIR");
    for source in [
        "cpp/chrome/session_form_fields.cpp",
        "cpp/chrome/session_form_settings.cpp",
        "cpp/chrome/session_form_advanced.cpp",
        "cpp/chrome/session_form_values.cpp",
        "cpp/chrome/modem_dialog.cpp",
        "cpp/chrome/modem_dialog.h",
        "cpp/chrome/session_form_widgets.h",
        "cpp/chrome/system_badge.cpp",
        "cpp/chrome/system_badge.h",
        "cpp/chrome/action_icons.cpp",
        "cpp/chrome/ui_capture.cpp",
        "cpp/chrome/ui_capture.h",
        "cpp/chrome/selftest_session_options.cpp",
        "cpp/chrome/selftest_session_options.h",
        "cpp/chrome/window_frame.cpp",
        "cpp/chrome/icon_shapes.cpp",
        "cpp/chrome/icon_shapes.h",
        "cpp/chrome/icon_canvas.h",
        "cpp/chrome/selftest_screen_lock.cpp",
        "cpp/chrome/selftest_screen_lock.h",
        "cpp/chrome/icon_shapes_actions.cpp",
        "cpp/chrome/icon_shapes_files.cpp",
        "cpp/chrome/login_dialog.cpp",
        "cpp/chrome/login_dialog.h",
        "cpp/chrome/screen_lock.cpp",
        "cpp/chrome/screen_lock.h",
        "cpp/chrome/screen_lock_auth.h",
        "cpp/chrome/screen_lock_settings.cpp",
        "cpp/chrome/screen_lock_settings.h",
        "cpp/chrome/screen_lock_idle.cpp",
        "cpp/chrome/window_frame.h",
        "cpp/chrome/elided_label.h",
        "cpp/chrome/transient_scrollbars.cpp",
        "cpp/chrome/transient_scrollbars.h",
        "cpp/chrome/selftest_chrome_layout.cpp",
        "cpp/chrome/selftest_chrome_layout.h",
        "cpp/chrome/selftest_c2.cpp",
        "cpp/chrome/selftest_shell.h",
        "cpp/main_window_context.cpp",
        "cpp/sftp/sftp_local.cpp",
        "cpp/sftp/file_item.h",
        "cpp/chrome/file_icons.cpp",
        "cpp/chrome/file_icons.h",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-env-changed=MSVC_INCLUDE");
    println!("cargo:rerun-if-env-changed=VCToolsInstallDir");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
}
