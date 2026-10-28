//! App-scoped prompt integration: no profile files or system settings are changed.
use super::store::session_config::LocalConfig;
use tiderm_core::error::{Error, Result};

pub struct LaunchOptions {
    pub program: String,
    pub args: Vec<String>,
    pub init: Option<tempfile::NamedTempFile>,
}

pub(super) fn isolated() -> bool {
    cfg!(test) || std::env::args().any(|arg| arg.ends_with("-selftest") || arg == "--safe-mode")
}

pub fn launch_options(local: &LocalConfig) -> Result<LaunchOptions> {
    let shell = if local.shell.is_empty() {
        "auto"
    } else {
        &local.shell
    };
    let program = if local.shell_path.is_empty() {
        shell
    } else {
        &local.shell_path
    };
    let family = if local.shell_path.is_empty() {
        shell
    } else {
        std::path::Path::new(program)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
    };
    let mut args = arguments(&local.args)?;
    if cfg!(windows) && args.is_empty() {
        if matches!(
            family.to_ascii_lowercase().as_str(),
            "auto" | "powershell" | "pwsh"
        ) {
            args = vec!["-NoExit".into(), "-Command".into(),
                r#"[Console]::Write(([char]27+']777;tideterm-history'+[char]7)); $global:ttHistoryId=0; $global:ttHistoryReady=$false; function global:prompt { $e=[char]27; if (-not $global:ttHistoryReady) { [Console]::WriteLine(('PowerShell '+$PSVersionTable.PSVersion)); [Console]::WriteLine(('Edition: '+$PSVersionTable.PSEdition)); [Console]::WriteLine() }; $h=Get-History -Count 1; if ($h) { if ($global:ttHistoryReady -and $h.Id -gt $global:ttHistoryId) { [Console]::Write(($e+']777;tideterm-command;'+[uri]::EscapeDataString($h.CommandLine)+[char]7)) }; $global:ttHistoryId=$h.Id }; $global:ttHistoryReady=$true; $p=$executionContext.SessionState.Path.CurrentLocation; if ($p.Provider.Name -eq 'FileSystem') { [Console]::Write(($e+']9;9;'+$p.ProviderPath+[char]7)) }; [Console]::Write(($e+']133;A'+[char]7)); $e+'[33mPS '+$p+$e+'[35m> '+$e+'[0m'+$e+']133;B'+[char]7 }"#.into()];
            // Console.Write uses the inherited OEM code page even in pwsh 7.
            // Our OSC directory/history messages and ConPTY streams use UTF-8.
            args.last_mut().expect("PowerShell startup command").insert_str(
                0,
                "[Console]::InputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); ",
            );
            // Existing self-test modes isolate profiles as well as app config.
            // Keep the production prompt integration, but never save test commands
            // into the user's PSReadLine history or execute their profile script.
            if isolated() {
                args.insert(0, "-NoProfile".into());
                let script = args
                    .last_mut()
                    .expect("PowerShell integration has a command");
                script.insert_str(0, "Import-Module PSReadLine -ErrorAction SilentlyContinue; if (Get-Module PSReadLine) { Set-PSReadLineOption -HistorySaveStyle SaveNothing }; ");
            }
        } else if family.eq_ignore_ascii_case("cmd") {
            args = vec![
                "/K".into(),
                r#"prompt $E]9;9;$P$E\$E[33m$P$E[35m$G$E[0m "#.into(),
            ];
            if isolated() {
                args.insert(0, "/D".into());
            }
        }
    }
    #[cfg(unix)]
    let init = bash::configure(program, &mut args, !isolated())?;
    #[cfg(not(unix))]
    let init = None;
    Ok(LaunchOptions {
        program: program.to_owned(),
        args,
        init,
    })
}

#[cfg(windows)]
fn arguments(raw: &str) -> Result<Vec<String>> {
    if raw.contains('\0') {
        return Err(Error::Pty("shell arguments contain NUL".into()));
    }
    // argv[0] has different quoting rules; prepend a dummy program, then discard it.
    let command: Vec<u16> = format!("tideterm-shell {raw}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut count = 0;
    // SAFETY: the input is NUL-terminated and alive for this call. The returned
    // count and NUL-terminated strings are owned by one Win32 allocation, which
    // is freed once with LocalFree after all strings have been copied.
    unsafe {
        let argv = windows_sys::Win32::UI::Shell::CommandLineToArgvW(command.as_ptr(), &mut count);
        if argv.is_null() {
            return Err(Error::Pty(format!(
                "parse shell arguments: {}",
                std::io::Error::last_os_error()
            )));
        }
        let args = (1..count as usize)
            .map(|i| {
                let arg = *argv.add(i);
                let mut len = 0;
                while *arg.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(arg, len))
            })
            .collect();
        windows_sys::Win32::Foundation::LocalFree(argv.cast());
        Ok(args)
    }
}

#[cfg(not(windows))]
fn arguments(raw: &str) -> Result<Vec<String>> {
    shell_words::split(raw).map_err(|error| Error::Pty(format!("parse shell arguments: {error}")))
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(unix)]
mod bash;
