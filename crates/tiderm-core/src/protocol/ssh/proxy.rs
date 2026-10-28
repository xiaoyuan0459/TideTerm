//! SOCKS5 / HTTP CONNECT negotiation on the SSH socket, bounded by one deadline.
use crate::error::{Error, Result};
use crate::session_options::ProxyOptions;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::time::Instant;

fn read(tcp: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Result<()> {
    for byte in buf {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Ssh("proxy negotiation timeout".into()));
        }
        tcp.set_read_timeout(Some(left))?;
        tcp.read_exact(std::slice::from_mut(byte))
            .map_err(|_| Error::Ssh("proxy reply failed or timed out".into()))?;
    }
    Ok(())
}

pub(super) fn negotiate(
    tcp: &mut TcpStream,
    p: &ProxyOptions,
    host: &str,
    port: u16,
    deadline: Instant,
) -> Result<()> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Error::Ssh("proxy negotiation timeout".into()));
    }
    tcp.set_write_timeout(Some(left))?;
    match p.kind {
        1 => socks(tcp, p, host, port, deadline),
        2 => http(tcp, p, host, port, deadline),
        _ => Err(Error::Ssh("invalid proxy type".into())),
    }?;
    tcp.set_read_timeout(None)?;
    tcp.set_write_timeout(None)?;
    Ok(())
}

fn socks(
    tcp: &mut TcpStream,
    p: &ProxyOptions,
    host: &str,
    port: u16,
    deadline: Instant,
) -> Result<()> {
    let auth = !p.username.is_empty();
    tcp.write_all(if auth { &[5, 1, 2] } else { &[5, 1, 0] })?;
    let mut reply = [0; 2];
    read(tcp, &mut reply, deadline)?;
    if reply != [5, if auth { 2 } else { 0 }] {
        return Err(Error::Ssh(
            "SOCKS5 proxy rejected authentication method".into(),
        ));
    }
    if auth {
        let mut credentials = vec![1, p.username.len() as u8];
        credentials.extend_from_slice(p.username.as_bytes());
        credentials.push(p.password.len() as u8);
        credentials.extend_from_slice(p.password.as_bytes());
        tcp.write_all(&credentials)?;
        read(tcp, &mut reply, deadline)?;
        if reply != [1, 0] {
            return Err(Error::Ssh("SOCKS5 proxy authentication failed".into()));
        }
    }
    let mut request = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend(ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend(ip.octets());
        }
        Err(_) if host.len() <= 255 => {
            request.extend([3, host.len() as u8]);
            request.extend(host.as_bytes());
        }
        Err(_) => return Err(Error::Ssh("SOCKS5 destination name too long".into())),
    }
    request.extend(port.to_be_bytes());
    tcp.write_all(&request)?;
    let mut header = [0; 4];
    read(tcp, &mut header, deadline)?;
    if header[0] != 5 || header[1] != 0 || header[2] != 0 {
        return Err(Error::Ssh(format!(
            "SOCKS5 CONNECT rejected (status {})",
            header[1]
        )));
    }
    let length = match header[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0];
            read(tcp, &mut len, deadline)?;
            len[0] as usize
        }
        _ => return Err(Error::Ssh("invalid SOCKS5 address type".into())),
    };
    read(tcp, &mut vec![0; length + 2], deadline)
}

fn http(
    tcp: &mut TcpStream,
    p: &ProxyOptions,
    host: &str,
    port: u16,
    deadline: Instant,
) -> Result<()> {
    use base64::Engine;
    if host.contains(['\r', '\n', '\0']) {
        return Err(Error::Ssh("invalid CONNECT host".into()));
    }
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let authorization = if p.username.is_empty() {
        String::new()
    } else {
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", p.username, p.password));
        format!("Proxy-Authorization: Basic {token}\r\n")
    };
    tcp.write_all(
        format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n{authorization}\r\n")
            .as_bytes(),
    )?;
    // Read exactly through the header; do not consume SSH banner bytes that follow it.
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() >= 16384 {
            return Err(Error::Ssh("HTTP proxy header exceeds 16 KiB".into()));
        }
        let mut byte = [0];
        read(tcp, &mut byte, deadline)?;
        response.push(byte[0]);
    }
    let line = std::str::from_utf8(&response)
        .ok()
        .and_then(|s| s.lines().next())
        .ok_or_else(|| Error::Ssh("invalid HTTP proxy reply".into()))?;
    let mut parts = line.split_whitespace();
    let version = parts.next().unwrap_or("");
    let code = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || !(200..300).contains(&code) {
        return Err(Error::Ssh(format!("HTTP CONNECT rejected (status {code})")));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
