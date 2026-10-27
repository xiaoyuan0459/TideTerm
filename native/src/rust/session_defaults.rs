//! Per-protocol options for future forms, using the existing atomic session store.
use super::store::{cfg_to_pod, kind_of, pod_to_cfg, session_config::SessionConfig, session_store};
use crate::bridge::session::session_ffi::SessCfg;
use tiderm_core::config::SessionStore;

fn path() -> std::path::PathBuf {
    super::store::defaults_path()
}

pub fn sess_defaults_get(kind: u8) -> SessCfg {
    let mut blank = SessionConfig::blank(kind_of(kind));
    if kind == 1 {
        blank.meta.system_shell = "linux".into();
    }
    let saved = SessionStore::load_at(path())
        .ok()
        .and_then(|store| store.find(&format!("defaults-{kind}")).cloned());
    if let Some(saved) = saved {
        blank = session_store::from_core(&saved);
        blank.id.clear();
    }
    cfg_to_pod(&blank)
}

fn sanitized(pod: &SessCfg) -> tiderm_core::config::SessionConfig {
    let input = pod_to_cfg(pod);
    let mut options = SessionConfig::blank(input.kind);
    options.meta.terminal_emulation = input.meta.terminal_emulation;
    options.meta.system_shell = input.meta.system_shell;
    options.meta.sftp_enabled = input.meta.sftp_enabled;
    options.meta.sysmon_enabled = input.meta.sysmon_enabled;
    options.meta.charset = input.meta.charset;
    options.meta.options = input.meta.options;
    options.meta.options.ssh.proxy.password.clear();
    if let (Some(ssh), Some(source)) = (&mut options.ssh, input.ssh) {
        ssh.auth = source.auth;
        ssh.private_key_path = source.private_key_path;
        ssh.port = source.port;
        ssh.connect_timeout_secs = source.connect_timeout_secs;
        ssh.reconnect_enabled = source.reconnect_enabled;
        ssh.max_reconnect_attempts = source.max_reconnect_attempts;
        ssh.reconnect_interval_secs = source.reconnect_interval_secs;
    }
    options.local = input.local;
    options.serial = input.serial;
    // A serial device and a working directory are destinations, not defaults.
    if let Some(serial) = &mut options.serial {
        serial.path.clear();
    }
    if let Some(local) = &mut options.local {
        local.working_directory.clear();
    }
    if let (Some(telnet), Some(source)) = (&mut options.telnet, input.telnet) {
        telnet.port = source.port;
        telnet.terminal_type = source.terminal_type;
    }
    if let (Some(tcp), Some(source)) = (&mut options.tcp, input.tcp) {
        tcp.port = source.port;
        tcp.terminal_type = source.terminal_type;
        tcp.data_mode = source.data_mode;
    }
    options.id = format!("defaults-{}", pod.kind);
    session_store::to_core(&options)
}

pub fn sess_defaults_save(pod: &SessCfg) -> String {
    if let Err(error) = super::session_advanced::validate(&pod_to_cfg(pod)) {
        return error;
    }
    if pod.kind >= 5 {
        return "unknown session protocol".into();
    }
    let result = (|| -> Result<(), String> {
        let mut store = SessionStore::load_at(path()).map_err(|e| e.to_string())?;
        store.upsert(sanitized(pod));
        store.save().map_err(|e| e.to_string())
    })();
    result.err().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preserve_options_but_exclude_hosts_accounts_and_credentials() {
        let mut pod = cfg_to_pod(&SessionConfig::blank(kind_of(1)));
        pod.ssh_host = "private.example".into();
        pod.ssh_user = "admin".into();
        pod.ssh_password = "secret-default".into();
        pod.ssh_passphrase = "secret-passphrase".into();
        pod.ssh_account_id = "vault-account".into();
        pod.ssh_remember = true;
        pod.sftp_enabled = false;
        pod.sysmon_enabled = true;
        pod.ssh_timeout_secs = 45;
        pod.ssh_reconnect_attempts = 9;
        pod.ssh_reconnect_interval = 12;
        pod.ssh_auth = 4;
        let saved = sanitized(&pod);
        let text = serde_json::to_string(&saved).unwrap();
        for secret in [
            "private.example",
            "admin",
            "secret-default",
            "secret-passphrase",
            "vault-account",
        ] {
            assert!(!text.contains(secret));
        }
        let restored = cfg_to_pod(&session_store::from_core(&saved));
        assert!(!restored.sftp_enabled);
        assert!(restored.sysmon_enabled);
        assert_eq!(restored.ssh_timeout_secs, 45);
        assert_eq!(restored.ssh_reconnect_attempts, 9);
        assert_eq!(restored.ssh_reconnect_interval, 12);
        assert_eq!(restored.ssh_auth, 4);
        assert!(!restored.ssh_remember);
    }
}
