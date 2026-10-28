//! Explicit one-shot introspection on the existing terminal; no exec channel.
use super::{
    shell_report_wire::{self as wire, Wire},
    shell_resolution,
    term::{try_with_tab, TermState},
};
use crate::bridge::completion::completion_ffi::{KubeScope, ShellReport};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use std::time::{Duration, Instant};

#[path = "shell_report_catalog.rs"]
mod catalog;
enum Query {
    Path,
    Lookup(String),
    Git,
    Docker(String),
    Kube(KubeScope),
    Names(String),
}
struct Pending {
    token: String,
    dialect: u8,
    query: Query,
    generation: u32,
    input_revision: u64,
    directory_revision: Option<u64>,
    deadline: Instant,
    sent: bool,
}
impl Pending {
    fn command(&self) -> Option<String> {
        match &self.query {
            Query::Names(prefix) => super::shell_names::command(&self.token, self.dialect, prefix),
            Query::Git => super::git_refs::command(&self.token, self.dialect),
            Query::Kube(scope) => super::kube_snapshot::command(&self.token, self.dialect, scope),
            Query::Docker(context) => {
                super::docker_snapshot::command(&self.token, self.dialect, context)
            }
            Query::Path => wire::command(&self.token, self.dialect),
            Query::Lookup(name) => shell_resolution::command(&self.token, self.dialect, name),
        }
    }
    fn decode(&self, body: &[u8]) -> Option<ShellReport> {
        let mut report = match &self.query {
            Query::Names(prefix) => {
                super::shell_names::decode(body, &self.token, self.dialect, prefix)?
            }
            Query::Git => super::git_refs::decode(body, &self.token, self.dialect)?,
            Query::Kube(scope) => {
                super::kube_snapshot::decode(body, &self.token, self.dialect, scope)?
            }
            Query::Docker(context) => {
                super::docker_snapshot::decode(body, &self.token, self.dialect, context)?
            }
            Query::Path => {
                let (path, platform) = wire::decode(body, &self.token, self.dialect)?;
                ShellReport {
                    path,
                    platform,
                    ..Default::default()
                }
            }
            Query::Lookup(name) => {
                let resolved = shell_resolution::decode(body, &self.token, self.dialect, name)?;
                ShellReport {
                    query: name.clone(),
                    kind: resolved.kind,
                    location: resolved.location,
                    platform: resolved.platform,
                    ..Default::default()
                }
            }
        };
        report.token = self.token.clone();
        report.command = self.command()?;
        Some(report)
    }
}
#[derive(Clone)]
struct Snapshot {
    report: ShellReport,
    at: Instant,
    generation: u32,
    input_revision: u64,
    directory_revision: Option<u64>,
}
#[derive(Default)]
pub struct Channel {
    catalog: catalog::CatalogPool,
    pending: Option<Pending>,
    snapshot: Option<Snapshot>,
    wire: Wire,
    state: u8,
}
impl Channel {
    pub(super) fn reported_context(
        &mut self,
        generation: u32,
        input_revision: u64,
        directory_revision: u64,
    ) -> (u8, u8, u8) {
        self.validate(generation, input_revision, directory_revision);
        let cached = self.catalog.context();
        let Some(snapshot) = self.snapshot.as_ref().filter(|_| self.state == 3) else {
            return cached;
        };
        let mut result = catalog::context(&snapshot.report);
        if result.0 == cached.0 {
            if result.1 == 0 {
                result.1 = cached.1;
            }
            if result.2 == 0 {
                result.2 = cached.2;
            }
        }
        result
    }
    pub(super) fn clear(&mut self) {
        self.clear_request();
        self.catalog = Default::default();
    }
    pub(super) fn clear_request(&mut self) {
        self.pending = None;
        self.snapshot = None;
        self.state = 0;
    }
    pub(super) fn output(&mut self, bytes: &[u8]) -> Vec<u8> {
        let (output, reports) = self.wire.feed(bytes);
        for body in reports {
            if let Some(p) = self
                .pending
                .as_ref()
                .filter(|p| p.sent && Instant::now() < p.deadline)
                && let Some(report) = p.decode(&body)
            {
                self.state = if report.name_error != 0 {
                    16
                } else if report.kube_error != 0 {
                    14
                } else if report.docker_error != 0 {
                    11
                } else if report.git_error != 0 {
                    10
                } else {
                    3
                };
                self.snapshot = Some(Snapshot {
                    report,
                    at: Instant::now(),
                    generation: p.generation,
                    input_revision: p.input_revision,
                    directory_revision: p.directory_revision,
                });
                if self.state == 3 {
                    self.catalog
                        .insert(self.snapshot.as_ref().expect("snapshot just stored"));
                }
                self.pending = None;
            }
        }
        output
    }
    fn validate(&mut self, generation: u32, input_revision: u64, directory_revision: u64) {
        self.catalog.validate(generation, directory_revision);
        if self.pending.as_ref().is_some_and(|p| {
            p.generation != generation
                || p.input_revision != input_revision
                || p.directory_revision
                    .is_some_and(|rev| rev != directory_revision)
        }) || self.snapshot.as_ref().is_some_and(|p| {
            p.generation != generation
                || p.input_revision != input_revision
                || p.directory_revision
                    .is_some_and(|rev| rev != directory_revision)
        }) {
            self.clear_request();
            self.state = 6;
        } else if self
            .pending
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.deadline)
        {
            self.clear_request();
            self.state = 4;
        } else if self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.at.elapsed() >= Duration::from_secs(60))
        {
            if let Some(snapshot) = &self.snapshot {
                self.catalog.retire(&snapshot.report.token, 7);
            }
            self.clear_request();
            self.state = 7;
        }
    }
    fn view(&self, source: u32) -> ShellReport {
        let mut result = self
            .snapshot
            .as_ref()
            .map(|s| s.report.clone())
            .unwrap_or_default();
        result.source = source;
        result.state = self.state;
        if let Some(p) = &self.pending {
            result.token = p.token.clone();
            match &p.query {
                Query::Names(prefix) => {
                    result.name_catalog = true;
                    result.name_prefix = prefix.clone();
                }
                Query::Lookup(name) => result.query = name.clone(),
                Query::Git => result.git = true,
                Query::Docker(context) => result.docker_context = context.clone(),
                Query::Kube(scope) => result.kube = scope.clone(),
                Query::Path => {}
            }
            result.command = p.command().unwrap_or_default();
        }
        if let Some(s) = &self.snapshot {
            result.age_seconds = s.at.elapsed().as_secs() as u32;
        }
        result.can_fill =
            self.state == 3 && !result.query.is_empty() && shell_resolution::accepts(result.kind);
        result
    }
}
fn available(st: &TermState) -> bool {
    matches!(
        st.config.kind,
        super::store::SessionKind::Local | super::store::SessionKind::Ssh
    ) && st.host.alive()
        && !st.reading
        && !st.host.modem.busy()
        && !st.pane.alternate_screen()
        && st.send_plan.is_none()
}
fn access(source: u32, f: impl FnOnce(&mut TermState) -> ShellReport) -> ShellReport {
    let locked = super::term_ops::input_locked();
    try_with_tab(source, |st| {
        if locked || !available(st) {
            st.context.shell.clear();
            return ShellReport {
                source,
                state: 6,
                ..Default::default()
            };
        }
        st.context
            .shell
            .validate(st.generation, st.input_revision, st.context.cwd_revision);
        f(st)
    })
    .unwrap_or_else(|| ShellReport {
        source,
        state: 6,
        ..Default::default()
    })
}
pub fn shell_report_prepare(source: u32, dialect: u8) -> ShellReport {
    prepare(source, dialect, Query::Path)
}
pub fn shell_names_prepare(source: u32, dialect: u8, prefix: &str) -> ShellReport {
    prepare(source, dialect, Query::Names(prefix.into()))
}
pub fn shell_resolution_prepare(source: u32, dialect: u8, name: &str) -> ShellReport {
    prepare(source, dialect, Query::Lookup(name.into()))
}
pub fn shell_git_prepare(source: u32, dialect: u8) -> ShellReport {
    prepare(source, dialect, Query::Git)
}
pub fn shell_docker_prepare(source: u32, dialect: u8, context: &str) -> ShellReport {
    prepare(source, dialect, Query::Docker(context.into()))
}
pub fn shell_kube_prepare(source: u32, dialect: u8, scope: &KubeScope) -> ShellReport {
    prepare(source, dialect, Query::Kube(scope.clone()))
}
fn prepare(source: u32, dialect: u8, query: Query) -> ShellReport {
    access(source, |st| {
        let provider = match &query {
            Query::Names(_) => Some(0),
            Query::Git => Some(1),
            Query::Docker(_) => Some(2),
            Query::Kube(scope) => Some(3 + usize::from(scope.resource)),
            _ => None,
        };
        if let Some(provider) = provider {
            st.context.shell.catalog.remove_provider(provider);
        }
        st.context.shell.clear_request();
        let invalid = match &query {
            Query::Names(prefix) if !super::shell_names::valid_prefix(prefix) => 17,
            Query::Lookup(name) if !shell_resolution::valid_name(name) => 9,
            Query::Docker(context) if !super::docker_snapshot::valid_context(context) => 12,
            Query::Kube(scope) if !super::kube_snapshot::valid_scope(scope, dialect) => 13,
            _ => 0,
        };
        if invalid != 0 {
            st.context.shell.state = invalid;
            return ShellReport {
                source,
                state: invalid,
                ..Default::default()
            };
        }
        if wire::command("", dialect).is_none() {
            st.context.shell.state = 5;
            return ShellReport {
                source,
                state: 5,
                ..Default::default()
            };
        }
        let mut random = [0_u8; 16];
        if OsRng.try_fill_bytes(&mut random).is_err() {
            st.context.shell.state = 8;
            return ShellReport {
                source,
                state: 8,
                ..Default::default()
            };
        }
        let token = random.iter().map(|b| format!("{b:02x}")).collect();
        let directory_revision =
            (!matches!(&query, Query::Path)).then_some(st.context.cwd_revision);
        st.context.shell.pending = Some(Pending {
            token,
            dialect,
            query,
            generation: st.generation,
            input_revision: st.input_revision,
            directory_revision,
            deadline: Instant::now() + Duration::from_secs(30),
            sent: false,
        });
        st.context.shell.state = 1;
        st.context.shell.view(source)
    })
}
pub fn shell_report_send(source: u32, token: &str) -> u8 {
    send(source, token, false)
}
pub fn shell_kube_send(source: u32, token: &str, credentials_confirmed: bool) -> u8 {
    send(source, token, credentials_confirmed)
}
fn send(source: u32, token: &str, credentials_confirmed: bool) -> u8 {
    access(source, |st| {
        if st.context.shell.pending.as_ref().is_some_and(|p| {
            !p.sent
                && p.token == token
                && matches!(&p.query, Query::Kube(_))
                && !credentials_confirmed
        }) {
            return ShellReport {
                source,
                state: 15,
                ..Default::default()
            };
        }
        if let Some(p) = st
            .context
            .shell
            .pending
            .as_mut()
            .filter(|p| !p.sent && p.token == token)
            && let Some(command) = p.command()
        {
            // Admission-aware FIFO, never synchronized input or Sender target groups.
            if st
                .host
                .queue_text(format!("{command}\r").as_bytes())
                .is_ok()
            {
                p.sent = true;
                p.deadline = Instant::now() + Duration::from_secs(15);
                st.context.shell.state = 2;
            } else {
                st.context.shell.clear();
                st.context.shell.state = 8;
            }
        } else {
            return ShellReport {
                source,
                state: 6,
                ..Default::default()
            };
        }
        st.context.shell.view(source)
    })
    .state
}
pub fn shell_resolution_draft(source: u32, receipt: &str) -> String {
    let mut draft = String::new();
    access(source, |st| {
        let report = st.context.shell.view(source);
        if report.can_fill && report.token == receipt {
            draft = report.query;
            st.context.shell.clear_request();
        }
        ShellReport::default()
    });
    draft
}
pub fn shell_report_poll(source: u32) -> ShellReport {
    access(source, |st| st.context.shell.view(source))
}
pub(super) fn shell_catalog_poll(source: u32, provider: u8) -> ShellReport {
    access(source, |st| st.context.shell.catalog.poll(source, provider))
}
pub(super) fn receipt_snapshot(source: u32, receipt: &str) -> ShellReport {
    access(source, |st| {
        let current = st.context.shell.view(source);
        if current.state == 3 && current.token == receipt {
            return current;
        }
        st.context
            .shell
            .catalog
            .receipt(source, receipt)
            .unwrap_or_default()
    })
}
pub fn shell_report_clear(source: u32) {
    let _ = try_with_tab(source, |st| st.context.shell.clear());
}
pub fn shell_report_cancel(source: u32) {
    let _ = try_with_tab(source, |st| {
        if let Some(snapshot) = &st.context.shell.snapshot {
            st.context.shell.catalog.retire(&snapshot.report.token, 0);
        }
        st.context.shell.clear_request();
    });
}
pub(super) fn filter_output(st: &mut TermState, bytes: &[u8]) -> Vec<u8> {
    if super::term_ops::input_locked() || !available(st) {
        st.context.shell.clear();
    }
    st.context
        .shell
        .validate(st.generation, st.input_revision, st.context.cwd_revision);
    st.context.shell.output(bytes)
}
#[cfg(test)]
#[path = "shell_docker_tests.rs"]
mod docker_tests;
#[cfg(test)]
#[path = "shell_git_tests.rs"]
mod git_tests;
#[cfg(test)]
#[path = "shell_kube_tests.rs"]
mod kube_tests;
#[cfg(test)]
#[path = "shell_names_tests.rs"]
mod names_tests;
#[cfg(test)]
#[path = "shell_report_remote_tests.rs"]
mod remote_tests;
#[cfg(test)]
#[path = "shell_resolution_tests.rs"]
mod resolution_tests;
#[cfg(test)]
#[path = "shell_suggestion_tests.rs"]
mod suggestion_tests;
#[cfg(test)]
#[path = "shell_report_tests.rs"]
mod tests;
