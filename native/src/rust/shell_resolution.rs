//! Exact-name introspection only; never execute the lookup target or import modules.
use super::shell_report_wire::decode_text;

pub(super) use tiderm_core::completion::literal_command_name as valid_name;
pub(super) fn accepts(kind: u8) -> bool {
    matches!(kind, 1..=3 | 5..=8)
}

pub(super) fn command(nonce: &str, dialect: u8, name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    match dialect {
        1 => Some(format!(
            r#"(LC_ALL=C; k=$(builtin type -t -- '{name}' 2>/dev/null) || k=missing; p=; if [[ $k == file ]]; then p=$(builtin type -p -- '{name}' 2>/dev/null) || p=; fi; if [[ ${{#p}} -le 4096 ]]; then builtin printf '\033]777;tideterm-env;2;{nonce};b;%s;{name};' "$k"; for ((i=0;i<${{#p}};i++)); do builtin printf '%02x' "'${{p:i:1}}"; done; builtin printf '\007'; fi)"#
        )),
        2 => Some(format!(
            r#"& {{ $PSModuleAutoLoadingPreference='None'; $c=$null; foreach ($v in $ExecutionContext.SessionState.InvokeCommand.GetCommands('{name}',[Management.Automation.CommandTypes]::All,$false)) {{ try {{ $c=$v }} finally {{ if ($foreach -is [IDisposable]) {{ $foreach.Dispose() }} }}; break }}; $k='missing'; $p=''; if ($c) {{ $k=$c.CommandType.ToString().ToLowerInvariant(); if ($k -eq 'application' -or $k -eq 'externalscript') {{ $p=$c.Path }} elseif ($k -eq 'cmdlet') {{ $p=$c.ModuleName }} }}; $t=[Text.Encoding]::UTF8.GetBytes([string]$p); if ($t.Length -le 4096) {{ $s=if ([IO.Path]::PathSeparator -eq ';') {{'w'}} else {{'p'}}; [Console]::Write(([char]27+']777;tideterm-env;2;{nonce};'+$s+';'+$k+';{name};'+[BitConverter]::ToString($t).Replace('-','')+[char]7)) }} }}"#
        )),
        _ => None,
    }
}

pub(super) struct Resolution {
    pub platform: u8,
    pub kind: u8,
    pub location: String,
}
pub(super) fn decode(body: &[u8], nonce: &str, dialect: u8, name: &str) -> Option<Resolution> {
    let text = std::str::from_utf8(body).ok()?;
    let mut fields = text.splitn(6, ';');
    if fields.next()? != "2" || fields.next()? != nonce {
        return None;
    }
    let platform = match (dialect, fields.next()?) {
        (1, "b") => 1,
        (2, "w") => 2,
        (2, "p") => 3,
        _ => return None,
    };
    let kind = match (dialect, fields.next()?) {
        (_, "missing") => 0,
        (_, "alias") => 1,
        (_, "function") => 2,
        (1, "builtin") => 3,
        (1, "keyword") => 4,
        (1, "file") | (2, "application") => 5,
        (2, "externalscript") => 6,
        (2, "cmdlet") => 7,
        (2, "filter") => 8,
        (2, "configuration") => 9,
        _ => return None,
    };
    if fields.next()? != name || !valid_name(name) {
        return None;
    }
    let location = decode_text(fields.next()?, 4096)?;
    if location.chars().any(char::is_control)
        || (matches!(kind, 5 | 6) && location.is_empty())
        || (!matches!(kind, 5..=7) && !location.is_empty())
    {
        return None;
    }
    Some(Resolution {
        platform,
        kind,
        location,
    })
}
