//! A private, session-owned rc file installs prompt notifications after the
//! user's rc file. Explicit argv and other shells retain their startup policy.
use std::{io::Write, path::Path};
use tiderm_core::error::{Error, Result};

pub(super) fn configure(
    program: &str,
    args: &mut Vec<String>,
    source_profile: bool,
) -> Result<Option<tempfile::NamedTempFile>> {
    let resolved = if program == "auto" {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    } else {
        program.to_owned()
    };
    if !args.is_empty() || Path::new(&resolved).file_name().is_none_or(|p| p != "bash") {
        return Ok(None);
    }
    let init = startup_file(source_profile)?;
    args.extend([
        "--rcfile".into(),
        init.path().to_string_lossy().into_owned(),
        "-i".into(),
    ]);
    Ok(Some(init))
}

fn startup_file(source_profile: bool) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .prefix("tideterm-bash-")
        .suffix(".bash")
        .tempfile()
        .map_err(|e| Error::Pty(format!("create Bash startup file: {e}")))?;
    let profile = if source_profile {
        "[[ ! -r $HOME/.bashrc ]] || source \"$HOME/.bashrc\"\n"
    } else {
        "unset HISTFILE\n"
    };
    file.write_all(profile.as_bytes())
        .and_then(|_| file.write_all(include_bytes!("bash_init.sh")))
        .and_then(|_| file.flush())
        .map_err(|e| Error::Pty(format!("write Bash startup file: {e}")))?;
    Ok(file)
}

#[cfg(test)]
mod tests;
