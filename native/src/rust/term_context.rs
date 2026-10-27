//! CWD is a session property. Parse bounded OSC bytes before terminal decoding,
//! including UTF-8 sequences split between transport chunks.
use super::store::session_config::{SessionConfig, SessionKind};
use super::term::{with_tab, TermState};

pub struct DirectoryContext {
    pub shell: super::shell_report::Channel,
    pub cwd: String,
    pub history: super::command_history::CommandHistory,
    pub(super) cwd_reported: bool,
    pub(super) cwd_revision: u64,
    osc: Vec<u8>,
    escaped: bool,
}

impl DirectoryContext {
    pub fn new(cfg: &SessionConfig) -> Self {
        Self {
            shell: Default::default(),
            cwd: if cfg.kind == SessionKind::Local {
                cfg.local
                    .as_ref()
                    .map(|c| c.working_directory.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            history: super::command_history::CommandHistory::default(),
            cwd_reported: false,
            cwd_revision: 0,
            osc: Vec::new(),
            escaped: false,
        }
    }

    fn feed(&mut self, bytes: &[u8], local: bool) {
        for &b in bytes {
            if self.osc.is_empty() {
                if self.escaped && b == b']' {
                    self.osc.extend_from_slice(b"\x1b]");
                }
                self.escaped = b == 0x1b;
                continue;
            }
            let done = b == 7 || (self.escaped && b == b'\\');
            self.osc.push(b);
            self.escaped = b == 0x1b;
            if done {
                if let Ok(seq) = std::str::from_utf8(&self.osc) {
                    self.history.notification(
                        seq.trim_start_matches("\x1b]")
                            .trim_end_matches(['\x07', '\\', '\x1b']),
                        local,
                    );
                    let cwd = if local {
                        super::local_cwd::from_osc(
                            seq.trim_start_matches("\x1b]")
                                .trim_end_matches(['\x07', '\\', '\x1b']),
                        )
                    } else {
                        // Remote authorities are permitted only for the remote SFTP pane.
                        let payload = seq
                            .trim_start_matches("\x1b]")
                            .trim_end_matches(['\x07', '\\', '\x1b']);
                        payload
                            .strip_prefix("7;")
                            .and_then(|s| url::Url::parse(s).ok())
                            .filter(|u| u.scheme() == "file")
                            .and_then(|u| {
                                percent_encoding::percent_decode_str(u.path())
                                    .decode_utf8()
                                    .ok()
                                    .map(|s| s.into_owned())
                            })
                            .or_else(|| {
                                payload
                                    .strip_prefix("9;9;")
                                    .map(|s| s.trim_matches('"').to_owned())
                            })
                    };
                    if let Some(cwd) = cwd.filter(|p| {
                        !p.contains(['\0', '\r', '\n'])
                            && if local {
                                std::path::Path::new(p).is_absolute()
                            } else {
                                p.starts_with('/')
                            }
                    }) {
                        if !self.cwd_reported || self.cwd != cwd {
                            self.cwd_revision = self.cwd_revision.wrapping_add(1);
                        }
                        self.cwd = cwd;
                        self.cwd_reported = true;
                    }
                }
                self.osc.clear();
                self.escaped = false;
            } else if self.osc.len() > 16384 {
                self.osc.clear();
                self.escaped = false;
            }
        }
    }
}

impl TermState {
    pub(super) fn accept_output(&mut self, bytes: &[u8]) {
        let bytes = super::shell_report::filter_output(self, bytes);
        let bytes = bytes.as_slice();
        self.preview_revision = self.preview_revision.wrapping_add(1);
        let line = |st: &Self| st.pane.output_line_number();
        let before = line(self);
        let text = self.codec.decode(bytes);
        if self.log.on() {
            self.log.append(text.as_bytes());
        }
        let (safe, bell) = self.codec.filter(&text, &self.config.meta.options);
        self.bell_pending |= bell;
        let (shell, system) = super::completion_shell::info(self);
        self.context
            .history
            .set_context(shell, system, &self.context.cwd);
        self.context
            .feed(&safe, self.config.kind == SessionKind::Local);
        self.pane.feed_bytes(&safe);
        if !bytes.is_empty() {
            self.annotations.received(before, line(self));
        }
    }
}

pub fn prepare_local(cfg: &mut SessionConfig) {
    if cfg.kind != SessionKind::Local {
        return;
    }
    let local = cfg
        .local
        .get_or_insert_with(|| SessionConfig::blank(SessionKind::Local).local.unwrap());
    if local.working_directory.is_empty() {
        // Self-tests already isolate profiles/configuration. Keep their shell and
        // file browser in the fixture cwd without spoofing the OS user profile.
        if super::local_shell::isolated()
            && let Ok(directory) = std::env::current_dir()
        {
            local.working_directory = directory.to_string_lossy().into_owned();
            return;
        }
        local.working_directory = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .unwrap_or_else(|_| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
    }
}

pub fn term_current_dir(idx: u32) -> String {
    with_tab(idx, |st| {
        sync_local_directory(st);
        st.context.cwd.clone()
    })
}

fn sync_local_directory(st: &mut TermState) {
    // An integrated shell may intentionally report a logical/symlink path.
    // Process snapshots fill the gap only for shells without OSC reporting.
    if st.config.kind == SessionKind::Local
        && !st.context.cwd_reported
        && let Some(path) = st.host.protocol().and_then(|p| p.local_working_directory())
    {
        let cwd = path.to_string_lossy().into_owned();
        if st.context.cwd != cwd {
            st.context.cwd_revision = st.context.cwd_revision.wrapping_add(1);
            st.context.cwd = cwd;
        }
    }
}

pub fn term_split_tab(idx: u32) -> u32 {
    let cfg = with_tab(idx, |st| {
        sync_local_directory(st);
        let mut cfg = st.config.clone();
        if let Some(local) = cfg.local.as_mut() {
            local.working_directory = st.context.cwd.clone();
        }
        cfg
    });
    if cfg.kind == SessionKind::Serial {
        return u32::MAX;
    }
    super::term_create::open_tab_with_config(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Transport chunks may bisect Unicode and OSC terminators. Existing vendor
    // string tests don't exercise this byte boundary or the remote SFTP policy.
    #[test]
    fn byte_split_notifications_and_remote_paths() {
        let cfg = SessionConfig::blank(SessionKind::Local);
        let mut context = DirectoryContext::new(&cfg);
        let path = if cfg!(windows) {
            "C:\\Users\\目录"
        } else {
            "/home/目录"
        };
        let seq = format!("\x1b]9;9;{path}\x1b\\");
        for byte in seq.as_bytes() {
            context.feed(&[*byte], true);
        }
        assert_eq!(context.cwd, path);
        let version = context.cwd_revision;
        context.feed(format!("\x1b]9;9;{path}/other\x07").as_bytes(), true);
        context.feed(format!("\x1b]9;9;{path}\x07").as_bytes(), true);
        assert!(context.cwd_revision > version);
        for byte in b"\x1b]777;tideterm-command;echo%20%E4%B8%AD%E6%96%87\x1b\\" {
            context.feed(&[*byte], true);
        }
        assert_eq!(context.history.snapshot(), vec!["echo 中文"]);
        // Percent-encoded UTF-8 can take three times the command byte budget.
        let command = "中".repeat(1300);
        let encoded =
            percent_encoding::utf8_percent_encode(&command, percent_encoding::NON_ALPHANUMERIC);
        context.feed(
            format!("\x1b]777;tideterm-command;{encoded}\x07").as_bytes(),
            true,
        );
        assert_eq!(context.history.snapshot().first(), Some(&command));
        context.feed(b"\x1b]7;file://server/home/user%20name\x07", false);
        assert_eq!(context.cwd, "/home/user name");
        context.feed(
            format!("\x1b]7;{}\x07", "a".repeat(18000)).as_bytes(),
            false,
        );
        context.feed(b"\x1b]7;file://server/tmp\x07", false);
        assert_eq!(context.cwd, "/tmp");
    }
}
