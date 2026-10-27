//! A separate file owner survives normal GUI exit until the external pager exits.
use std::path::{Path, PathBuf};
use std::process::Command;

fn owned_path(path: &Path) -> Result<tempfile::TempPath, String> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid history file")?;
    if !name.starts_with("tideterm-history-")
        || !name.ends_with(".txt")
        || path.parent().and_then(|p| p.canonicalize().ok())
            != std::env::temp_dir().canonicalize().ok()
        || !path.is_file()
    {
        return Err("not an owned temporary history file".into());
    }
    tempfile::TempPath::try_from_path(path).map_err(|e| e.to_string())
}

fn execute(args: &[String]) -> Result<i32, String> {
    if args.len() != 5 {
        return Err("invalid history pager arguments".into());
    }
    let kind: u8 = args[2].parse().map_err(|_| "invalid pager kind")?;
    let file = owned_path(Path::new(&args[3]))?;
    let executable = PathBuf::from(&args[4]);
    if !executable.is_absolute() || !executable.is_file() || !matches!(kind, 1 | 2) {
        return Err("pager executable unavailable".into());
    }
    let mut command;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command = Command::new(executable);
        command.creation_flags(0x10);
    }
    #[cfg(not(windows))]
    {
        command = Command::new("xterm");
        command.arg("-e").arg(executable);
    }
    command
        .env("LESSSECURE", "1")
        .env("LESSOPEN", "")
        .env("LESSCLOSE", "");
    if kind == 1 {
        command.args(["-R", "--"]);
    } else {
        command.args([
            "-R",
            "-M",
            "-n",
            "-u",
            "NONE",
            "-i",
            "NONE",
            "--cmd",
            "set nomodeline",
            "--",
        ]);
    }
    // Windows creates the pager's standard handles in its new console. xterm
    // owns its own PTY; its launcher does not need the parent's standard handles.
    #[cfg(not(windows))]
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let status = command
        .arg(file.as_os_str())
        .status()
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}

pub fn run(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("--history-pager-helper") {
        return None;
    }
    Some(match execute(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!(
                "history pager: {}",
                error.chars().take(1024).collect::<String>()
            );
            2
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_pager_cleans_owned_history_and_rejects_other_files() {
        let file = tempfile::Builder::new()
            .prefix("tideterm-history-")
            .suffix(".txt")
            .tempfile()
            .unwrap();
        let (handle, path) = file.keep().unwrap();
        drop(handle);
        let args = vec![
            "native".into(),
            "--history-pager-helper".into(),
            "1".into(),
            path.to_string_lossy().into_owned(),
            path.with_extension("missing")
                .to_string_lossy()
                .into_owned(),
        ];
        assert!(execute(&args).is_err());
        assert!(!path.exists());
        let other = tempfile::NamedTempFile::new().unwrap();
        assert!(owned_path(other.path()).is_err());
        assert!(
            other.path().exists(),
            "an unrelated file must never be adopted for cleanup"
        );
    }
}
