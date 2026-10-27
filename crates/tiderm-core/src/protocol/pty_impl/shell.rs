//! Executable discovery and named-shell policy, separate from PTY lifetime/IO.
use crate::error::{Error, Result};
use std::path::Path;

/// Git's bin entry point supplies its toolchain PATH; do not select WSL bash.
pub(super) fn git_bash_path() -> Option<String> {
    let mut roots = Vec::new();
    if let Ok(pf) = std::env::var("ProgramFiles") {
        roots.push(format!("{pf}\\Git"));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        roots.push(format!("{pf86}\\Git"));
    }
    if let Ok(lad) = std::env::var("LOCALAPPDATA") {
        roots.push(format!("{lad}\\Programs\\Git"));
    }
    roots
        .iter()
        .flat_map(|root| {
            [
                format!(r"{root}\bin\bash.exe"),
                format!(r"{root}\usr\bin\bash.exe"),
            ]
        })
        .find(|path| Path::new(path).is_file())
}

#[cfg(windows)]
fn pwsh_path() -> Option<String> {
    // Respect the user's PATH (including ZIP/MSIX installs), then MSI's stable
    // installation location. Never silently substitute Windows PowerShell 5.1.
    let mut paths = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("pwsh.exe"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for root in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(root) {
            paths.push(
                Path::new(&root)
                    .join("PowerShell")
                    .join("7")
                    .join("pwsh.exe"),
            );
        }
    }
    paths
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn windows_powershell() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe")
}

pub(super) fn resolve(shell: &str) -> Result<(String, Vec<String>)> {
    match shell {
        // Preserve existing auto policy and all persisted explicit selections.
        "auto" if cfg!(windows) => Ok((windows_powershell(), vec![])),
        "auto" => Ok((
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()),
            vec![],
        )),
        "powershell" => Ok((windows_powershell(), vec![])),
        #[cfg(windows)]
        "pwsh" => pwsh_path().map(|path| (path, vec![])).ok_or_else(|| {
            Error::Pty("PowerShell 7 (pwsh) is not installed or discoverable".into())
        }),
        #[cfg(not(windows))]
        "pwsh" => Ok(("pwsh".into(), vec![])),
        "cmd" => Ok(("cmd.exe".into(), vec![])),
        "git-bash" if cfg!(windows) => git_bash_path()
            .map(|path| (path, vec!["--login".into(), "-i".into()]))
            .ok_or_else(|| Error::Pty("Git Bash is not installed or discoverable".into())),
        "git-bash" => Err(Error::Pty("Git Bash is only available on Windows".into())),
        "bash" if cfg!(windows) => match git_bash_path() {
            Some(path) => Ok((path, vec!["--login".into(), "-i".into()])),
            // Keep the legacy bash fallback; explicit git-bash never falls into WSL.
            None => Ok(("bash".into(), vec!["-i".into()])),
        },
        "bash" => Ok(("bash".into(), vec![])),
        "zsh" | "fish" if cfg!(windows) => Ok((shell.into(), vec!["-i".into()])),
        "zsh" | "fish" => Ok((shell.into(), vec![])),
        // The session form's executable-path override is passed as one program,
        // including paths with spaces; arguments remain a separate vector.
        path if Path::new(path).is_absolute() => Ok((path.into(), vec![])),
        other => Err(Error::Pty(format!("unknown shell: {other}"))),
    }
}
