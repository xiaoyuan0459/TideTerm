//! Shared parser for the address bar and quick connection dialog.
use crate::bridge::context::context_ffi::QuickTarget;

pub fn context_parse_address(address: &str) -> QuickTarget {
    let fail = |error: &str| QuickTarget {
        ok: false,
        kind: 1,
        host: String::new(),
        port: 0,
        user: String::new(),
        error: error.into(),
    };
    let address = address.trim();
    if address.is_empty() || address.len() > 2048 || address.contains(['\r', '\n', '\0']) {
        return fail("请输入有效地址");
    }
    let input = if address.contains("://") {
        address.to_owned()
    } else if address.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("ssh://[{address}]")
    } else {
        format!("ssh://{address}")
    };
    let Ok(uri) = url::Url::parse(&input) else {
        return fail("地址或端口无效");
    };
    let (kind, default_port) = match uri.scheme() {
        "ssh" => (1, 22),
        "telnet" => (2, 23),
        "tcp" => (4, 23),
        "serial" => (3, 0),
        "shell" | "local" => (0, 0),
        _ => return fail("支持 shell、ssh、telnet、tcp 和 serial 协议"),
    };
    if kind == 3
        && uri.host_str().is_none()
        && uri.path().starts_with('/')
        && uri.query().is_none()
        && uri.fragment().is_none()
    {
        return QuickTarget {
            ok: true,
            kind,
            host: uri.path().into(),
            port: 0,
            user: String::new(),
            error: String::new(),
        };
    }
    if uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
        || !matches!(uri.path(), "" | "/")
    {
        return fail("地址不能包含密码、查询参数或路径");
    }
    let host = uri
        .host_str()
        .unwrap_or_default()
        .trim_matches(['[', ']'])
        .to_owned();
    if host.is_empty() {
        return fail("地址不能为空");
    }
    if kind == 0 && !["auto", "powershell", "cmd", "bash", "zsh", "fish"].contains(&host.as_str()) {
        return fail("未知的本地 Shell");
    }
    let port = uri.port().unwrap_or(default_port);
    if kind != 0 && kind != 3 && port == 0 {
        return fail("端口范围为 1–65535");
    }
    let Ok(user) = percent_encoding::percent_decode_str(uri.username()).decode_utf8() else {
        return fail("用户名编码无效");
    };
    if user.contains(['\0', '\r', '\n']) {
        return fail("用户名无效");
    }
    QuickTarget {
        ok: true,
        kind,
        host,
        port: port.into(),
        user: user.into_owned(),
        error: String::new(),
    }
}

pub fn context_open_shell(shell: &str) -> u32 {
    use super::store::session_config::{SessionConfig, SessionKind};
    let parsed = context_parse_address(&format!("shell://{shell}"));
    if !parsed.ok {
        return u32::MAX;
    }
    let mut cfg = SessionConfig::blank(SessionKind::Local);
    if let Some(local) = cfg.local.as_mut() {
        local.shell = parsed.host;
    }
    super::term_create::open_tab_with_config(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Contract: both entry points interpret hosts identically, never truncating ports
    // or confusing IPv6 colons. The old dialog's last-colon splitter could do both.
    #[test]
    fn address_targets_and_invalid_input() {
        for (input, kind, host, port, user) in [
            ("192.168.31.188", 1, "192.168.31.188", 22, ""),
            ("ssh://admin@host:2222", 1, "host", 2222, "admin"),
            ("telnet://host", 2, "host", 23, ""),
            ("tcp://[::1]:8080", 4, "::1", 8080, ""),
            ("::1", 1, "::1", 22, ""),
            ("shell://cmd", 0, "cmd", 0, ""),
        ] {
            let target = context_parse_address(input);
            assert!(target.ok, "{input}: {}", target.error);
            assert_eq!(
                (
                    target.kind,
                    target.host.as_str(),
                    target.port,
                    target.user.as_str()
                ),
                (kind, host, port, user)
            );
        }
        for input in [
            "",
            "host:65536",
            "host:0",
            "http://host",
            "ssh://u:secret@host",
            "ssh://host/path",
            "shell://oops",
        ] {
            assert!(!context_parse_address(input).ok, "{input}");
        }
    }
}
