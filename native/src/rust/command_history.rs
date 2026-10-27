//! Per-terminal, bounded, in-memory commands. Never record arbitrary key streams.
use super::term::with_tab;
use std::collections::VecDeque;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Default)]
struct Scope {
    shell: u8,
    system: u8,
    directory: Arc<str>,
}
struct Record {
    command: String,
    scope: Scope,
}
#[derive(Default)]
pub(super) struct Usage {
    pub count: u32,
    pub recent: usize,
    pub same_directory: bool,
}
#[derive(Default)]
pub(super) struct WeightedHistory {
    pub commands: Vec<String>,
    pub usage: HashMap<String, Usage>,
}

#[derive(Default)]
pub struct CommandHistory {
    pub integrated: bool,
    pub revision: u64,
    commands: VecDeque<Record>,
    scope: Scope,
}

impl CommandHistory {
    pub(super) fn set_context(&mut self, shell: u8, system: u8, directory: &str) {
        let directory = if directory.len() <= 1024 {
            directory
        } else {
            ""
        };
        if self.scope.shell != shell
            || self.scope.system != system
            || self.scope.directory.as_ref() != directory
        {
            self.scope = Scope {
                shell,
                system,
                directory: Arc::from(directory),
            };
        }
    }
    pub(super) fn weighted(&self, shell: u8, system: u8, directory: &str) -> WeightedHistory {
        let mut result = WeightedHistory::default();
        for (recent, record) in self.commands.iter().rev().enumerate() {
            if (shell != 0 && record.scope.shell != 0 && record.scope.shell != shell)
                || (system != 0 && record.scope.system != 0 && record.scope.system != system)
            {
                continue;
            }
            let usage = result
                .usage
                .entry(record.command.clone())
                .or_insert_with(|| {
                    result.commands.push(record.command.clone());
                    Usage {
                        recent,
                        ..Default::default()
                    }
                });
            usage.count += 1;
            usage.same_directory |=
                !directory.is_empty() && record.scope.directory.as_ref() == directory;
        }
        result
    }
    pub fn record(&mut self, command: &str) {
        let command = command.trim();
        if command.is_empty() || command.len() > 4096 || command.contains('\0') {
            return;
        }
        if self.commands.len() == 500 {
            self.commands.pop_front();
        }
        self.commands.push_back(Record {
            command: command.to_owned(),
            scope: self.scope.clone(),
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn notification(&mut self, payload: &str, local: bool) {
        if !local {
            return;
        }
        if payload == "777;tideterm-history" {
            // Startup announces the hook before we know whether this shell
            // actually reports commands. Keep prompt-line fallback available.
            return;
        }
        if let Some(encoded) = payload.strip_prefix("777;tideterm-command;")
            && let Ok(command) = percent_encoding::percent_decode_str(encoded).decode_utf8()
        {
            // The first command may already have been captured on Enter.
            if self.integrated
                || self
                    .commands
                    .back()
                    .is_none_or(|last| last.command != command.as_ref())
            {
                self.record(&command);
            }
            self.integrated = true;
        }
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.commands
            .iter()
            .rev()
            .map(|record| record.command.clone())
            .collect()
    }

    pub fn clear(&mut self) {
        self.commands.clear();
        self.revision = self.revision.wrapping_add(1);
    }
}

pub fn history_revision(idx: u32) -> u64 {
    with_tab(idx, |st| st.context.history.revision)
}
pub fn history_commands(idx: u32) -> Vec<String> {
    with_tab(idx, |st| st.context.history.snapshot())
}
pub fn history_clear(idx: u32) {
    with_tab(idx, |st| st.context.history.clear());
}

/// Close only an opted-in startup range at the existing recognized shell prompt.
pub(super) fn observe_prompt(st: &mut super::term::TermState) -> bool {
    let shell = super::completion_shell::configured_kind(st);
    st.pane.command_startup_pending()
        && st
            .pane
            .command_line()
            .is_some_and(|line| prompt_input_for_shell(&line, shell).is_some())
        && st.pane.command_startup_finish()
}

/// Fallback for shells without integration: inspect the submitted prompt line,
/// rather than retaining every key (which would also retain password input).
pub fn submitted(st: &mut super::term::TermState, bytes: &[u8]) {
    use super::store::session_config::SessionKind;
    if !matches!(
        st.config.kind,
        SessionKind::Local | SessionKind::Ssh | SessionKind::Telnet
    ) {
        return;
    }
    if !bytes.ends_with(b"\r") && !bytes.ends_with(b"\n") {
        return;
    }
    let (shell, system) = super::completion_shell::info(st);
    st.context
        .history
        .set_context(shell, system, &st.context.cwd);
    let existing = if st.pane.has_command_input_markers() {
        if !st.pane.command_input_active() {
            return;
        }
        let local = super::completion_shell::configured_kind(st);
        (local != 0)
            .then(|| {
                st.pane.command_line().and_then(|line| {
                    prompt_input_for_shell(&line, local).map(|input| input.trim().to_owned())
                })
            })
            .flatten()
            .or_else(|| st.pane.command_input_line())
    } else {
        st.pane.command_line().and_then(|line| {
            prompt_input_for_shell(&line, shell).map(|input| input.trim().to_owned())
        })
    };
    let Some(existing) = existing else {
        return;
    };
    let Ok(typed) = std::str::from_utf8(bytes) else {
        return;
    };
    let typed = typed.trim_end_matches(['\r', '\n']);
    // Embedded escapes are editor/navigation controls, not a command payload.
    if typed
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return;
    }
    let command = if typed.is_empty() {
        existing
    } else {
        format!("{existing}{typed}")
    };
    st.pane.command_submitted(&command);
    if !st.context.history.integrated {
        st.context.history.record(&command);
    }
}

/// Preserve the terminal editor draft; submission history trims it separately.
pub(super) fn prompt_input(line: &str) -> Option<&str> {
    if let Some(at) = line.find('>') {
        let prefix = line[..at].trim_start_matches("PS ");
        if prefix.as_bytes().get(1) == Some(&b':')
            && prefix
                .as_bytes()
                .get(2)
                .is_some_and(|b| matches!(*b, b'\\' | b'/'))
        {
            let input = &line[at + 1..];
            return Some(input.strip_prefix(' ').unwrap_or(input));
        }
    }
    for marker in ["$ ", "# "] {
        if let Some(at) = line.find(marker) {
            let prefix = &line[..at];
            if prefix.contains('@') || prefix.starts_with('[') || prefix.contains(':') {
                return Some(&line[at + marker.len()..]);
            }
        }
    }
    // Empty canonical POSIX prompt: typed payload supplied by a send/paste action.
    if line.ends_with(['$', '#']) && (line.contains('@') || line.contains(':')) {
        return Some("");
    }
    None
}

/// Recognize only the launcher's conventional prompts when no OSC anchor exists.
/// A bare `>`/`%` is ambiguous (continuation/password/program input), so require
/// the conventional user/host or directory prefix and a known matching family.
pub(super) fn prompt_input_for_shell(line: &str, shell: u8) -> Option<&str> {
    if let Some(input) = prompt_input(line) {
        return Some(input);
    }
    let marker = match shell {
        4 => "% ",
        5 => "> ",
        _ => return None,
    };
    let at = line.find(marker)?;
    let prefix = &line[..at];
    if !prefix.contains('@')
        && !(shell == 5 && (prefix.starts_with('~') || prefix.starts_with('/')))
    {
        return None;
    }
    Some(&line[at + marker.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_records_local_and_ssh_prompts_after_startup_marker() {
        use super::super::store::session_config::SessionKind;
        use tiderm_terminal::{SoftwareRenderer, TerminalPane};
        let tab = super::super::term::term_new_tab();
        super::super::term::with_tab(tab, |st| {
            st.context
                .history
                .notification("777;tideterm-history", true);
            st.pane = TerminalPane::new(12, 10, Box::new(SoftwareRenderer::new()));
            st.pane
                .feed_bytes(b"C:\\Users\\long-user-name>echo WRAPPED_HISTORY\x1b[15D");
            submitted(st, b"\r");
            assert_eq!(st.context.history.snapshot(), vec!["echo WRAPPED_HISTORY"]);
            assert_eq!(st.pane.command_blocks()[0].title, "echo WRAPPED_HISTORY");
            st.pane.feed_bytes(b"\x1b[2J\x1b[HPassword: ");
            submitted(st, b"secret\r");
            st.pane.feed_bytes(b"\x1b[?1049hC:\\Users\\user>echo TUI");
            submitted(st, b"\r");
            assert_eq!(st.context.history.snapshot(), vec!["echo WRAPPED_HISTORY"]);
            st.config.kind = SessionKind::Ssh;
            st.pane = TerminalPane::new(20, 10, Box::new(SoftwareRenderer::new()));
            st.pane.feed_bytes(b"xiao@xiao:~$ pwd");
            submitted(st, b"\r");
            assert_eq!(
                st.context.history.snapshot(),
                vec!["pwd", "echo WRAPPED_HISTORY"]
            );
        });
        super::super::term::with_tab(tab, |st| {
            st.config.kind = SessionKind::Local;
            for (shell, prompt, command) in [
                ("zsh", "user@host ~ % print ZSH", "print ZSH"),
                ("fish", "user@host ~> printf FISH", "printf FISH"),
            ] {
                st.config.local =
                    super::super::store::session_config::SessionConfig::blank(SessionKind::Local)
                        .local;
                st.config.local.as_mut().unwrap().shell = shell.into();
                st.pane = TerminalPane::new(80, 10, Box::new(SoftwareRenderer::new()));
                st.pane.feed(prompt);
                submitted(st, b"\r");
                assert_eq!(st.context.history.snapshot()[0], command);
            }
            assert_eq!(prompt_input_for_shell("user@host % print x", 0), None);
            assert_eq!(prompt_input_for_shell("Password> secret", 5), None);
            assert_eq!(prompt_input_for_shell("> secret", 5), None);
        });
        super::super::term::term_close_tab(tab);
    }
    #[test]
    fn notifications_bound_commands_and_reject_remote_reports() {
        let mut history = CommandHistory::default();
        history.notification("777;tideterm-command;secret", false);
        assert!(history.commands.is_empty());
        history.notification("777;tideterm-history", true);
        history.notification(
            "777;tideterm-command;echo%20%E4%B8%AD%E6%96%87%3B%20pwd",
            true,
        );
        assert_eq!(history.snapshot().last().unwrap(), "echo 中文; pwd");
        history.record(&"a".repeat(4097));
        assert_eq!(history.commands.len(), 1);
        for i in 0..510 {
            history.record(&format!("echo {i}"));
        }
        assert_eq!(history.commands.len(), 500);
        assert_eq!(history.snapshot().last().unwrap(), "echo 10");
        let revision = history.revision;
        history.clear();
        assert!(history.commands.is_empty());
        assert!(history.revision > revision);
    }
}
