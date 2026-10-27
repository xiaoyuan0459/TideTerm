//! Passive session metadata only. Never send a Shell command to classify input.
use super::{store::SessionKind, term::TermState};

pub(super) fn info(st: &mut TermState) -> (u8, u8) {
    let (reported, reported_shell, reported_system) = st.context.shell.reported_context(
        st.generation,
        st.input_revision,
        st.context.cwd_revision,
    );
    if st.config.kind != SessionKind::Local {
        return (
            if reported_shell != 0 {
                reported_shell
            } else {
                match reported {
                    1 => 0,
                    2 | 3 => 2,
                    _ => 0,
                }
            },
            reported_system,
        );
    }
    let kind = configured_kind(st);
    // A different visible prompt can indicate a nested interpreter. Do not keep
    // applying the launcher's grammar to it or infer a remote Shell from a title.
    if let Some(line) = st.pane.command_prefix() {
        let ps = line.starts_with("PS ");
        let posix = super::command_history::prompt_input(&line).is_some()
            && (line.contains("$ ") || line.contains("# ") || line.ends_with(['$', '#']));
        if (ps && kind != 2) || (posix && !matches!(kind, 1 | 4..=6)) {
            return (0, 0);
        }
    }
    (
        kind,
        if reported_system != 0
            && ((reported == 1 && matches!(kind, 1 | 4..=6))
                || (matches!(reported, 2 | 3) && kind == 2))
        {
            reported_system
        } else if kind == 0
            || (cfg!(windows)
                && kind == 1
                && st
                    .config
                    .local
                    .as_ref()
                    .is_some_and(|v| v.shell_path.is_empty() && v.shell == "bash"))
        {
            0
        } else if cfg!(windows) {
            1
        } else if cfg!(target_os = "linux") {
            2
        } else if cfg!(target_os = "macos") {
            3
        } else {
            0
        },
    )
}

/// Launcher metadata is a hint only; used for conservative default-prompt fallback.
pub(super) fn configured_kind(st: &TermState) -> u8 {
    if st.config.kind != SessionKind::Local {
        return 0;
    }
    let Some(local) = &st.config.local else {
        return 0;
    };
    let family = if local.shell_path.is_empty() {
        local.shell.as_str()
    } else {
        std::path::Path::new(&local.shell_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
    };
    let family = if matches!(family, "" | "auto") && !cfg!(windows) {
        std::env::var("SHELL")
            .ok()
            .and_then(|p| {
                std::path::Path::new(&p)
                    .file_stem()
                    .map(|v| v.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    } else {
        family.to_ascii_lowercase()
    };
    match family.as_str() {
        "" | "auto" if cfg!(windows) => 2,
        "pwsh" | "powershell" => 2,
        "cmd" => 3,
        "bash" | "git-bash" => 1,
        "zsh" => 4,
        "fish" => 5,
        "sh" => 6,
        _ => 0,
    }
}
