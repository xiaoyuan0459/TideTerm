//! Explicit transport lifecycle for the Qt menu. A reconnect gets a fresh
//! event uid, so late output or Exit from the old read loop cannot enter it.

use super::session::SessionHost;
use super::store::session_config::{SessionConfig, SessionKind, SshAuth};
use super::term::with_tab;

/// 测试专用：把本线程的标签序号前推 `n`（只动计数器，不建 pane）。
///
/// 隔离跨测例串台：`NEXT_ID` 是 `thread_local`（cargo 一条测例一条线程 ⇒ 每线程第一个
/// 标签都是 `tab-0`），而事件总线是**进程全局**的 —— 上一个测例收尾时才发出的 `Exit`
/// 会被下一个测例的第一次 `term_poll` 领走，按 uid 命中它的新标签并把 `note` 写成
/// 「会话已结束」（状态码 5，看着像交付失败）。生产侧标签全在 GUI 线程上开、id 单调
/// 不复用，所以这**不是产品缺陷**，纯粹是测试隔离。用法见 `term_net_tests::net_serial`。
#[cfg(test)]
pub fn reserve_tab_ids(n: u32) {
    super::term::NEXT_ID.with(|c| c.set(c.get() + n));
}

pub fn active_config(idx: u32) -> SessionConfig {
    with_tab(idx, |st| st.config.clone())
}

pub fn term_session_name(idx: u32) -> String {
    with_tab(idx, |st| super::term_diag::display_title(st))
}

pub fn term_session_url(idx: u32) -> String {
    with_tab(idx, |st| {
        let cfg = &st.config;
        let authority = |host: &str, port: u16| {
            if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            }
        };
        match cfg.kind {
            SessionKind::Local => format!(
                "shell://{}",
                cfg.local
                    .as_ref()
                    .map(|c| if c.shell.is_empty() {
                        "powershell"
                    } else {
                        c.shell.as_str()
                    })
                    .unwrap_or("auto")
            ),
            SessionKind::Ssh => cfg
                .ssh
                .as_ref()
                .map(|s| {
                    format!(
                        "ssh://{}{}",
                        if s.username.is_empty() {
                            String::new()
                        } else {
                            format!("{}@", s.username)
                        },
                        authority(&s.host, s.port)
                    )
                })
                .unwrap_or_default(),
            SessionKind::Telnet => cfg
                .telnet
                .as_ref()
                .map(|s| format!("telnet://{}", authority(&s.host, s.port)))
                .unwrap_or_default(),
            SessionKind::Tcp => cfg
                .tcp
                .as_ref()
                .map(|s| format!("tcp://{}", authority(&s.host, s.port)))
                .unwrap_or_default(),
            SessionKind::Serial => cfg
                .serial
                .as_ref()
                .map(|s| format!("serial://{}", s.path))
                .unwrap_or_default(),
        }
    })
}

pub fn term_disconnect_tab(idx: u32) -> bool {
    with_tab(idx, |st| {
        st.context.shell.clear();
        st.history_export = None;
        st.tmux_request = None;
        st.system_monitor = Default::default();
        super::sftp::close_session(&st.uid);
        st.host.disconnect();
        st.manual_stop = true;
        st.auto_retry_at = None;
        // R-19：手动断开是用户动作不是错误 —— note 承载状态文案（角标 4=已断开）。
        st.error.set(false);
        *st.note.borrow_mut() = "会话已断开".into();
        st.redraw_pending = true;
    });
    true
}

pub fn term_reconnect_tab(idx: u32) -> bool {
    restart_tab(idx, false)
}

pub(super) fn term_auto_reconnect_tab(idx: u32) -> bool {
    restart_tab(idx, true)
}

fn restart_tab(idx: u32, automatic: bool) -> bool {
    with_tab(idx, |st| {
        st.history_export = None;
        st.tmux_request = None;
        st.context.shell.clear();
        st.system_monitor = Default::default();
        super::sftp::close_session(&st.uid);
        st.host.disconnect();
        st.generation += 1;
        let uid = format!("tab-{idx}-g{}", st.generation);
        let (cols, rows) = st.pane.grid_size();
        // 勾选记住的会话以凭据库为准：编辑改密码保存后，原地重连直接用新密码
        // （否则标签内存里还是打开时注入的旧密码）。
        crate::rust::vault::refresh_saved_credentials(&mut st.config);
        st.host = if automatic {
            st.auto_attempts += 1;
            SessionHost::start_auto(uid.clone(), &st.config, cols as u16, rows as u16)
        } else {
            st.auto_attempts = 0;
            SessionHost::start(uid.clone(), &st.config, cols as u16, rows as u16)
        };
        st.uid = uid;
        st.codec = super::session_codec::SessionCodec::new(&st.config.meta.charset);
        st.bell_pending = false;
        super::term_modes::apply(&mut st.pane, &st.config.meta.options);
        st.manual_stop = false;
        st.auto_retry_at = None;
        // 重连进度是状态文案（connecting），不是错误。
        st.error.set(false);
        *st.note.borrow_mut() = if automatic {
            format!(
                "自动重连 {}/{}",
                st.auto_attempts,
                st.config
                    .ssh
                    .as_ref()
                    .map_or(0, |s| s.max_reconnect_attempts)
            )
        } else {
            String::new()
        };
        st.redraw_pending = true;
    });
    true
}

pub(super) fn auto_retry_enabled(cfg: &SessionConfig) -> bool {
    cfg.kind == SessionKind::Ssh
        && cfg.ssh.as_ref().is_some_and(|s| {
            s.reconnect_enabled
                && matches!(s.auth, SshAuth::Password | SshAuth::Key | SshAuth::Agent)
        })
}

#[derive(Clone, Copy)]
pub(super) struct RetryInput<'a> {
    pub enabled: bool,
    pub manual_stop: bool,
    pub attempts: u32,
    pub max_attempts: u32,
    pub connected: bool,
    pub alive: bool,
    pub exit_code: Option<u32>,
    pub status: &'a str,
}

pub(super) fn should_retry(input: RetryInput<'_>) -> bool {
    input.enabled
        && !input.manual_stop
        && input.attempts < input.max_attempts
        && !input.status.contains("host key")
        && ((input.connected && !input.alive && input.exit_code.is_none())
            || (input.attempts > 0 && !input.connected))
}

#[cfg(test)]
mod auto_tests {
    use super::{should_retry, RetryInput};

    #[test]
    fn retries_link_loss_but_not_clean_exit_manual_stop_or_changed_key() {
        let base = RetryInput {
            enabled: true,
            manual_stop: false,
            attempts: 0,
            max_attempts: 3,
            connected: true,
            alive: false,
            exit_code: None,
            status: "会话已结束",
        };
        assert!(should_retry(base));
        assert!(!should_retry(RetryInput {
            exit_code: Some(0),
            ..base
        }));
        assert!(!should_retry(RetryInput {
            manual_stop: true,
            ..base
        }));
        assert!(!should_retry(RetryInput {
            attempts: 1,
            connected: false,
            status: "changed host key",
            ..base
        }));
        assert!(should_retry(RetryInput {
            attempts: 1,
            connected: false,
            status: "network timeout",
            ..base
        }));
        assert!(!should_retry(RetryInput {
            attempts: 3,
            connected: false,
            status: "network timeout",
            ..base
        }));
    }
}
