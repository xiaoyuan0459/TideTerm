//! SSH authentication — password, public-key, and keyboard-interactive.
//!
//! v0.10 (libssh2 migration): direct calls to `ssh2::Session::userauth_*`.
//! All auth runs on a worker thread (`spawn_blocking`) because libssh2 is blocking.

use crate::auth::KbSyncBackend;
use crate::error::{Error, Result};
use crate::event::ChallengeField;
use crate::protocol::ssh::config::SshConfig;
use crate::protocol::ssh::handler::infer_challenge_field_type;
use ssh2::{KeyboardInteractivePrompt, Prompt};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Authenticate a libssh2 session after the SSH handshake (password/public-key).
///
/// Keyboard-interactive is handled separately by the caller (it needs a
/// frontend prompt bridge).
pub fn authenticate(session: &ssh2::Session, config: &SshConfig) -> Result<()> {
    match &config.auth {
        super::config::SshAuth::Agent => session
            .userauth_agent(&config.username)
            .map_err(|e| Error::Ssh(format!("agent auth: {e}"))),
        super::config::SshAuth::Password { password } => session
            .userauth_password(&config.username, password.as_str())
            .map_err(|e| Error::Ssh(format!("password auth: {e}"))),
        super::config::SshAuth::PublicKey {
            private_key_path,
            passphrase,
        } => {
            let key_path = Path::new(private_key_path);
            session
                .userauth_pubkey_file(&config.username, None, key_path, passphrase.as_deref())
                .map_err(|e| Error::Ssh(format!("publickey auth: {e}")))
        }
        super::config::SshAuth::KeyboardInteractive => Err(Error::Ssh(
            "keyboard-interactive not supported by this helper; use the prompter path".into(),
        )),
    }
}

/// A `KeyboardInteractivePrompt` that bridges libssh2's synchronous prompt
/// callback to TideTerm over a `std::sync::mpsc` channel.
///
/// The callback emits the challenge through a de-shelled [`KbSyncBackend`] and
/// blocks (up to 60 s) on `recv_timeout` for the frontend's response. An empty
/// response vector cancels the auth attempt.
pub struct SshPrompter {
    pub backend: Arc<dyn KbSyncBackend>,
    pub session_id: String,
    pub rx: Receiver<Vec<String>>,
}

impl KeyboardInteractivePrompt for SshPrompter {
    fn prompt<'b>(&mut self, name: &str, instruction: &str, prompts: &[Prompt<'b>]) -> Vec<String> {
        let fields: Vec<ChallengeField> = prompts
            .iter()
            .map(|p| ChallengeField {
                name: p
                    .text
                    .trim_end_matches(':')
                    .to_lowercase()
                    .replace(' ', "_"),
                prompt: p.text.to_string(),
                field_type: infer_challenge_field_type(&p.text, p.echo),
                echo: p.echo,
            })
            .collect();

        let _ = self
            .backend
            .emit(&self.session_id, name, instruction, &fields);

        match self.rx.recv_timeout(Duration::from_secs(60)) {
            Ok(responses) => responses,
            Err(_) => {
                tracing::warn!("kbd-interactive: prompt timed out or cancelled");
                prompts.iter().map(|_| String::new()).collect()
            }
        }
    }
}

/// Helper to build the prompter channel pair for keyboard-interactive auth.
/// Returns `(sender, receiver)`; the sender is stored on the session so the
/// frontend can deliver responses via `respond_keyboard_interactive`.
pub fn kbd_channel() -> (Sender<Vec<String>>, Receiver<Vec<String>>) {
    std::sync::mpsc::channel()
}

/// Open a PTY + shell channel on an already-authenticated libssh2 session.
/// `term` 为 PTY 请求的终端模拟类型（空串回退 xterm-256color）。
pub fn open_pty_shell(
    session: &ssh2::Session,
    cols: u16,
    rows: u16,
    term: &str,
    options: &crate::session_options::SshOptions,
) -> Result<super::raw_channel::Channel> {
    let mut channel = super::raw_channel::Channel::open(session)
        .map_err(|e| Error::Ssh(format!("open channel: {e}")))?;

    if options.agent_forwarding || options.x11_enabled {
        let (forwarding, x11) = super::forwarding::Forwarding::start(session, options)?;
        channel.retain_forwarding(forwarding);
        if options.agent_forwarding {
            channel
                .request_agent()
                .map_err(|e| Error::Ssh(format!("request Agent forwarding: {e}")))?;
        }
        if let Some(x11) = x11 {
            channel
                .request_x11(&x11.cookie_hex(), x11.screen)
                .map_err(|e| Error::Ssh(format!("request X11 forwarding: {e}")))?;
        }
    }

    channel
        .request_pty(
            if term.is_empty() {
                "xterm-256color"
            } else {
                term
            },
            cols,
            rows,
        )
        .map_err(|e| Error::Ssh(format!("request pty: {e}")))?;

    channel
        .shell()
        .map_err(|e| Error::Ssh(format!("request shell: {e}")))?;

    Ok(channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kbd_channel_round_trip() {
        let (tx, rx) = kbd_channel();
        tx.send(vec!["p1".into()]).expect("send");
        assert_eq!(rx.recv().expect("recv"), vec!["p1".to_string()]);
    }
}
