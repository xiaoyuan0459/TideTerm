//! Presentation names for executable aliases; never feed these back into launch commands.

pub fn executable_name(command: &str) -> &str {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    if name
        .get(name.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
    {
        &name[..name.len() - 4]
    } else {
        name
    }
}

pub fn standard_shell_name(command: &str) -> Option<&'static str> {
    let name = executable_name(command);
    [
        ("powershell", "PowerShell"),
        ("pwsh", "PowerShell"),
        ("cmd", "Command Prompt"),
        ("bash", "Bash"),
        ("zsh", "Zsh"),
        ("fish", "Fish"),
        ("wsl", "WSL"),
        ("git-bash", "Git Bash"),
    ]
    .into_iter()
    .find_map(|(alias, display)| name.eq_ignore_ascii_case(alias).then_some(display))
}

pub fn shell_display_name(command: &str) -> String {
    standard_shell_name(command)
        .unwrap_or_else(|| executable_name(command))
        .to_owned()
}
