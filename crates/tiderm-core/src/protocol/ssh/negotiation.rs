//! Validate explicit algorithm preferences against this libssh2 build before handshake.
use crate::error::{Error, Result};
use crate::session_options::SshOptions;
use ssh2::MethodType;

pub(super) fn configure(session: &ssh2::Session, options: &SshOptions) -> Result<()> {
    session.set_compress(options.compression);
    for (kind, list) in [
        (MethodType::Kex, &options.kex),
        (MethodType::HostKey, &options.host_keys),
        (MethodType::CryptCs, &options.ciphers),
        (MethodType::CryptSc, &options.ciphers),
        (MethodType::MacCs, &options.macs),
        (MethodType::MacSc, &options.macs),
    ] {
        if list.is_empty() {
            continue;
        }
        let supported = session
            .supported_algs(kind)
            .map_err(|e| Error::Ssh(format!("algorithm query: {e}")))?;
        if list.split(',').any(|name| !supported.contains(&name)) {
            return Err(Error::Ssh("unsupported SSH algorithm preference".into()));
        }
        session
            .method_pref(kind, list)
            .map_err(|e| Error::Ssh(format!("algorithm preference: {e}")))?;
    }
    session.set_keepalive(true, options.keepalive_secs);
    Ok(())
}

pub fn validate_options(options: &SshOptions) -> Result<()> {
    let session = ssh2::Session::new().map_err(|e| Error::Ssh(e.to_string()))?;
    configure(&session, options)
}

pub fn algorithms(group: u8) -> Vec<String> {
    let kind = match group {
        0 => MethodType::CryptCs,
        1 => MethodType::Kex,
        2 => MethodType::MacCs,
        _ => MethodType::HostKey,
    };
    ssh2::Session::new()
        .and_then(|s| s.supported_algs(kind))
        .unwrap_or_default()
        .into_iter()
        .map(String::from)
        .collect()
}
