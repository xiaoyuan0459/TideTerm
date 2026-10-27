//! X11 endpoint and setup authentication. No X server cookie crosses SSH.
use crate::error::{Error, Result};
use crate::session_options::SshOptions;
use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct X11 {
    pub host: String,
    pub display: u16,
    pub screen: u32,
    #[cfg(unix)]
    pub local: bool,
    pub fake: [u8; 16],
    real: Vec<u8>,
}
impl X11 {
    pub fn load(options: &SshOptions) -> Result<Self> {
        let display = if options.x11_display.is_empty() {
            std::env::var("DISPLAY").unwrap_or_else(|_| "127.0.0.1:0".into())
        } else {
            options.x11_display.clone()
        };
        let (host, tail) = display
            .rsplit_once(':')
            .ok_or_else(|| Error::Ssh("X11 display must be host:display.screen".into()))?;
        let (number, screen) = tail.split_once('.').unwrap_or((tail, "0"));
        let number = number
            .parse::<u16>()
            .ok()
            .filter(|n| *n <= 59535)
            .ok_or_else(|| Error::Ssh("invalid X11 display number".into()))?;
        let screen = screen
            .parse::<u32>()
            .ok()
            .filter(|n| *n <= 65535)
            .ok_or_else(|| Error::Ssh("invalid X11 screen".into()))?;
        let local = host.is_empty() || host == "unix";
        let host: String = if local {
            "127.0.0.1".into()
        } else {
            host.trim_matches(['[', ']']).into()
        };
        let real = if options.x11_no_auth {
            Vec::new()
        } else {
            let path = if !options.x11_auth_file.is_empty() {
                PathBuf::from(&options.x11_auth_file)
            } else if let Some(path) = std::env::var_os("XAUTHORITY") {
                path.into()
            } else {
                dirs::home_dir()
                    .ok_or_else(|| Error::Ssh("X11 requires XAUTHORITY".into()))?
                    .join(".Xauthority")
            };
            let file = std::fs::File::open(path)
                .map_err(|e| Error::Ssh(format!("read Xauthority: {e}")))?;
            if file.metadata()?.len() > 1 << 20 {
                return Err(Error::Ssh("Xauthority exceeds 1 MiB".into()));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take((1 << 20) + 1).read_to_end(&mut bytes)?;
            authority(&bytes, number, &host, local)?
        };
        Ok(Self {
            host,
            display: number,
            screen,
            #[cfg(unix)]
            local,
            fake: *uuid::Uuid::new_v4().as_bytes(),
            real,
        })
    }
    pub fn cookie_hex(&self) -> String {
        self.fake.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    /// Validate the remote's fake cookie and replace it with local authorization.
    pub fn setup(&self, bytes: &[u8]) -> Result<Option<(usize, Vec<u8>)>> {
        if bytes.len() < 12 {
            return Ok(None);
        }
        let little = match bytes[0] {
            b'l' => true,
            b'B' => false,
            _ => return Err(Error::Ssh("invalid X11 byte order".into())),
        };
        let word = |offset| {
            let b = [bytes[offset], bytes[offset + 1]];
            if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        if word(2) != 11 {
            return Err(Error::Ssh("unsupported X11 version".into()));
        }
        let name_len = word(6) as usize;
        let data_len = word(8) as usize;
        let padded = |n: usize| (n + 3) & !3;
        let end = 12 + padded(name_len) + padded(data_len);
        if end > 4096 {
            return Err(Error::Ssh("X11 setup exceeds 4 KiB".into()));
        }
        if bytes.len() < end {
            return Ok(None);
        }
        let start = 12 + padded(name_len);
        if &bytes[12..12 + name_len] != b"MIT-MAGIC-COOKIE-1"
            || bytes[start..start + data_len] != self.fake
        {
            return Err(Error::Ssh("X11 authorization rejected".into()));
        }
        let protocol = if self.real.is_empty() {
            &b""[..]
        } else {
            &b"MIT-MAGIC-COOKIE-1"[..]
        };
        let mut out = bytes[..12].to_vec();
        for (offset, len) in [(6, protocol.len()), (8, self.real.len())] {
            let len = if little {
                (len as u16).to_le_bytes()
            } else {
                (len as u16).to_be_bytes()
            };
            out[offset..offset + 2].copy_from_slice(&len);
        }
        out.extend_from_slice(protocol);
        out.resize(12 + padded(protocol.len()), 0);
        out.extend_from_slice(&self.real);
        out.resize(12 + padded(protocol.len()) + padded(self.real.len()), 0);
        Ok(Some((end, out)))
    }
}
fn authority(mut input: &[u8], display: u16, host: &str, local: bool) -> Result<Vec<u8>> {
    fn field<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
        let len = u16::from_be_bytes(input.get(..2)?.try_into().ok()?) as usize;
        let value = input.get(2..2 + len)?;
        *input = &input[2 + len..];
        Some(value)
    }
    while input.len() >= 2 {
        let family = u16::from_be_bytes([input[0], input[1]]);
        input = &input[2..];
        let record = (|| {
            Some((
                field(&mut input)?,
                field(&mut input)?,
                field(&mut input)?,
                field(&mut input)?,
            ))
        })();
        let Some((address, number, name, cookie)) = record else {
            break;
        };
        let local_host = local || host == "localhost" || host == "127.0.0.1" || host == "::1";
        let matches = family == 65535
            || (family == 256 && local_host)
            || (family == 0
                && host
                    .parse::<std::net::Ipv4Addr>()
                    .is_ok_and(|ip| address == ip.octets()))
            || (family == 6
                && host
                    .parse::<std::net::Ipv6Addr>()
                    .is_ok_and(|ip| address == ip.octets()));
        if matches
            && number == display.to_string().as_bytes()
            && name == b"MIT-MAGIC-COOKIE-1"
            && cookie.len() == 16
        {
            return Ok(cookie.to_vec());
        }
    }
    Err(Error::Ssh(
        "no matching MIT-MAGIC-COOKIE-1 in Xauthority".into(),
    ))
}
