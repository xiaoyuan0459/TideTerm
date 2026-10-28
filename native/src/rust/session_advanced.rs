//! Advanced form options have one shared backend owner.
use crate::bridge::session::session_ffi::SessOptions;
use tiderm_core::session_options::SessionOptions;

pub fn from_pod(p: &SessOptions) -> SessionOptions {
    let mut o = SessionOptions {
        bell_visual: p.bell_visual,
        bell_sound: p.bell_sound,
        bell_interval_secs: p.bell_interval_secs,
        auto_wrap: p.auto_wrap,
        newline_mode: p.newline_mode,
        app_cursor: p.app_cursor,
        app_keypad: p.app_keypad,
        cursor_style: p.cursor_style,
        scrollback_mb: p.scrollback_mb,
        backspace_del: p.backspace_del,
        delete_mode: p.delete_mode,
        enter_mode: p.enter_mode,
        remote_clipboard: p.remote_clipboard,
        remote_title: p.remote_title,
        log_enabled: p.log_enabled,
        copy_on_select: p.copy_on_select,
        path_word_selection: p.path_word_selection,
        mark_save_on_close: p.mark_save_on_close,
        right_click_paste: p.right_click_paste,
        modem_timeout_secs: p.modem_timeout_secs,
        ..Default::default()
    };
    o.ssh.compression = p.compression;
    o.ssh.keepalive_secs = p.keepalive_secs;
    o.ssh.watchdog_secs = p.watchdog_secs;
    o.ssh.proxy.kind = p.proxy_kind;
    o.ssh.proxy.host = p.proxy_host.clone();
    o.ssh.proxy.port = p.proxy_port as u16;
    o.ssh.proxy.username = p.proxy_user.clone();
    o.ssh.proxy.password = p.proxy_password.clone();
    o.ssh.ciphers = p.ciphers.clone();
    o.ssh.kex = p.kex.clone();
    o.ssh.macs = p.macs.clone();
    o.ssh.host_keys = p.host_keys.clone();
    o.ssh.agent_forwarding = p.agent_forwarding;
    o.ssh.x11_enabled = p.x11_enabled;
    o.ssh.x11_display = p.x11_display.clone();
    o.ssh.x11_auth_file = p.x11_auth_file.clone();
    o.ssh.x11_no_auth = p.x11_no_auth;
    o.sftp_initial_dir = p.sftp_initial_dir.clone();
    o.sftp_show_hidden = p.sftp_show_hidden;
    o.sftp_follow_cwd = p.sftp_follow_cwd;
    o.sftp_repair_names = p.sftp_repair_names;
    o.sftp_preserve_modified = p.sftp_preserve_modified;
    o.sftp_preserve_accessed = p.sftp_preserve_accessed;
    o.ssh.sftp_file_mode = p.sftp_file_mode;
    o.ssh.sftp_dir_mode = p.sftp_dir_mode;
    o
}

pub fn to_pod(o: &SessionOptions) -> SessOptions {
    SessOptions {
        bell_visual: o.bell_visual,
        bell_sound: o.bell_sound,
        bell_interval_secs: o.bell_interval_secs,
        auto_wrap: o.auto_wrap,
        newline_mode: o.newline_mode,
        app_cursor: o.app_cursor,
        app_keypad: o.app_keypad,
        cursor_style: o.cursor_style,
        scrollback_mb: o.scrollback_mb,
        backspace_del: o.backspace_del,
        delete_mode: o.delete_mode,
        enter_mode: o.enter_mode,
        remote_clipboard: o.remote_clipboard,
        remote_title: o.remote_title,
        log_enabled: o.log_enabled,
        copy_on_select: o.copy_on_select,
        path_word_selection: o.path_word_selection,
        mark_save_on_close: o.mark_save_on_close,
        right_click_paste: o.right_click_paste,
        modem_timeout_secs: o.modem_timeout_secs,
        compression: o.ssh.compression,
        keepalive_secs: o.ssh.keepalive_secs,
        watchdog_secs: o.ssh.watchdog_secs,
        proxy_kind: o.ssh.proxy.kind,
        proxy_host: o.ssh.proxy.host.clone(),
        proxy_port: o.ssh.proxy.port as u32,
        proxy_user: o.ssh.proxy.username.clone(),
        proxy_password: String::new(),
        ciphers: o.ssh.ciphers.clone(),
        kex: o.ssh.kex.clone(),
        macs: o.ssh.macs.clone(),
        host_keys: o.ssh.host_keys.clone(),
        agent_forwarding: o.ssh.agent_forwarding,
        x11_enabled: o.ssh.x11_enabled,
        x11_display: o.ssh.x11_display.clone(),
        x11_auth_file: o.ssh.x11_auth_file.clone(),
        x11_no_auth: o.ssh.x11_no_auth,
        sftp_initial_dir: o.sftp_initial_dir.clone(),
        sftp_show_hidden: o.sftp_show_hidden,
        sftp_follow_cwd: o.sftp_follow_cwd,
        sftp_repair_names: o.sftp_repair_names,
        sftp_preserve_modified: o.sftp_preserve_modified,
        sftp_preserve_accessed: o.sftp_preserve_accessed,
        sftp_file_mode: o.ssh.sftp_file_mode,
        sftp_dir_mode: o.ssh.sftp_dir_mode,
    }
}

pub fn context_take_bell(idx: u32) -> bool {
    super::term::try_with_tab(idx, |st| std::mem::take(&mut st.bell_pending)).unwrap_or(false)
}
pub fn context_right_click_paste(idx: u32) -> bool {
    super::term::try_with_tab(idx, |st| st.config.meta.options.right_click_paste).unwrap_or(false)
}
pub fn sess_ssh_algorithms(group: u8) -> Vec<String> {
    tiderm_core::protocol::ssh::algorithms(group)
}

pub fn sess_validate(p: &crate::bridge::session::session_ffi::SessCfg) -> String {
    if p.options.proxy_port > u16::MAX as u32 {
        return "invalid proxy port".into();
    }
    validate(&super::store::pod_to_cfg(p))
        .err()
        .unwrap_or_default()
}

pub fn validate(c: &super::store::session_config::SessionConfig) -> Result<(), String> {
    c.meta.options.validate()?;
    if c.kind == super::store::SessionKind::Ssh {
        tiderm_core::protocol::ssh::validate_options(&c.meta.options.ssh)
            .map_err(|e| e.to_string())?;
        if let Some(ssh) = &c.ssh
            && (ssh.max_reconnect_attempts > 100
                || ssh.reconnect_interval_secs == 0
                || ssh.reconnect_interval_secs > 3600)
        {
            return Err("invalid reconnect policy".into());
        }
    }
    super::session_codec::SessionCodec::encoding(&c.meta.charset)?;
    Ok(())
}
