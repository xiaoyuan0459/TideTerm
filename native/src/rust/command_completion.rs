//! Pure draft suggestions: querying and validating never call a transport writer.
use super::{command_library, term::try_with_tab};
use crate::bridge::completion::completion_ffi::{
    CommandSuggestion, CompletionBatch, CompletionContext, ShellReport,
};

pub fn completion_context(source: u32) -> CompletionContext {
    let catalog_revision = command_library::revision();
    if super::term_ops::input_locked() {
        return CompletionContext::default();
    }
    try_with_tab(source, |st| {
        let (shell_kind, system) = super::completion_shell::info(st);
        CompletionContext {
            shell_kind,
            system,
            source,
            generation: st.generation,
            input_revision: st.input_revision,
            history_revision: st.context.history.revision,
            catalog_revision,
            cwd: st.context.cwd.clone(),
            directory_revision: st.context.cwd_revision,
            sftp_enabled: st.config.kind == super::store::SessionKind::Ssh
                && st.config.meta.sftp_enabled,
            ready: st.host.alive() && !st.reading && !st.host.modem.busy(),
        }
    })
    .unwrap_or_default()
}
pub fn completion_valid(snapshot: &CompletionContext) -> bool {
    snapshot.ready && *snapshot == completion_context(snapshot.source)
}
pub fn completion_query(source: u32, query: &str) -> CompletionBatch {
    completion_environment_query(source, query, 0)
}
pub fn completion_environment_query(source: u32, query: &str, owner: u64) -> CompletionBatch {
    let context = completion_context(source);
    let mut batch = CompletionBatch {
        context,
        items: vec![],
        rule_error: false,
        shell: ShellReport::default(),
    };
    if !batch.context.ready || query.len() > 4096 || query.chars().any(char::is_control) {
        return batch;
    }
    let history = try_with_tab(source, |st| {
        st.context.history.weighted(
            batch.context.shell_kind,
            batch.context.system,
            &batch.context.cwd,
        )
    })
    .unwrap_or_default();
    let catalog = command_library::command_catalog();
    let mut entries = catalog.entries;
    entries.sort_by(|a, b| (&a.group, &a.name, a.id).cmp(&(&b.group, &b.name, b.id)));
    let candidates = history
        .commands
        .into_iter()
        .filter(|s| !s.chars().any(char::is_control))
        .map(|command| CommandSuggestion {
            kind: 0,
            label: String::new(),
            command,
            ..Default::default()
        })
        .chain(entries.into_iter().map(|e| CommandSuggestion {
            kind: 1,
            label: if e.group.is_empty() {
                e.name
            } else {
                format!("{} / {}", e.group, e.name)
            },
            command: e.command,
            user_id: e.id,
            is_template: e.is_template,
            ..Default::default()
        }));
    let rules = match if batch.context.shell_kind == 0 {
        Ok(vec![])
    } else {
        super::command_rules::suggest(query)
    } {
        Ok(items) => items,
        Err(()) => {
            batch.rule_error = true;
            vec![]
        }
    };
    let environment = super::command_environment::ready_items(owner, &batch.context, query);
    batch.shell = shell_snapshot(source, query);
    let git = super::git_refs::suggestions(&batch.shell, query);
    let docker = super::docker_snapshot::suggestions(&batch.shell, query);
    let kube = super::kube_snapshot::suggestions(&batch.shell, query);
    let names = super::shell_names::suggestions(&batch.shell, query);
    let shell = batch.shell.can_fill.then(|| CommandSuggestion {
        kind: 7,
        command: batch.shell.query.clone(),
        ..Default::default()
    });
    // Prefer the checked Shell receipt over an identical literal history entry.
    batch.items = ranked(
        shell
            .into_iter()
            .chain(git)
            .chain(docker)
            .chain(kube)
            .chain(names)
            .chain(candidates)
            .chain(rules)
            .chain(super::command_vocabulary::suggestions(
                &batch.context,
                query,
            ))
            .chain(environment),
        query,
        &history.usage,
    );
    batch
}
fn shell_snapshot(source: u32, query: &str) -> ShellReport {
    if let Some(provider) = super::kube_snapshot::provider(query) {
        let mut report = super::shell_report::shell_catalog_poll(source, provider);
        report.command.clear();
        return report;
    }
    if super::docker_snapshot::supported(query) {
        let mut report = super::shell_report::shell_catalog_poll(source, 3);
        report.command.clear();
        return report;
    }
    if super::git_refs::supported(query) {
        let mut report = super::shell_report::shell_catalog_poll(source, 2);
        report.command.clear();
        return report;
    }
    if !query.is_empty() && !super::shell_resolution::valid_name(query) {
        return ShellReport::default();
    }
    let mut report = super::shell_report::shell_report_poll(source);
    if report.state != 3
        || report.query.is_empty()
        || !report
            .query
            .to_ascii_lowercase()
            .starts_with(&query.to_ascii_lowercase())
    {
        return super::shell_report::shell_catalog_poll(source, 1);
    }
    report.command.clear();
    report.path.clear();
    report
}
pub fn completion_shell_valid(context: &CompletionContext, receipt: &str) -> bool {
    if receipt.is_empty() || !completion_valid(context) {
        return false;
    }
    let report = super::shell_report::receipt_snapshot(context.source, receipt);
    report.state == 3
        && report.token == receipt
        && (report.name_catalog
            || report.git
            || !report.docker_context.is_empty()
            || !report.kube.config.is_empty()
            || !report.query.is_empty())
}
pub fn completion_kube_draft(
    context: &CompletionContext,
    receipt: &str,
    query: &str,
    uid: &str,
) -> String {
    if !completion_shell_valid(context, receipt) {
        return String::new();
    }
    super::kube_snapshot::suggestions(
        &super::shell_report::receipt_snapshot(context.source, receipt),
        query,
    )
    .into_iter()
    .find(|i| i.reference == uid)
    .map(|i| i.command)
    .unwrap_or_default()
}
pub fn completion_docker_draft(
    context: &CompletionContext,
    receipt: &str,
    query: &str,
    id: &str,
) -> String {
    if !completion_shell_valid(context, receipt) {
        return String::new();
    }
    let report = super::shell_report::receipt_snapshot(context.source, receipt);
    super::docker_snapshot::suggestions(&report, query)
        .into_iter()
        .find(|s| s.reference == id)
        .map(|s| s.command)
        .unwrap_or_default()
}
pub fn completion_git_draft(
    context: &CompletionContext,
    receipt: &str,
    query: &str,
    reference: &str,
) -> String {
    if !completion_shell_valid(context, receipt) {
        return String::new();
    }
    let report = super::shell_report::receipt_snapshot(context.source, receipt);
    super::git_refs::suggestions(&report, query)
        .into_iter()
        .find(|s| s.reference == reference)
        .map(|s| s.command)
        .unwrap_or_default()
}
pub fn completion_shell_draft(context: &CompletionContext, receipt: &str) -> String {
    if !completion_valid(context) {
        return String::new();
    }
    // Terminal state is thread-local: no yield between context validation and the
    // report channel's guarded, once-only consumption. This never writes bytes.
    super::shell_report::shell_resolution_draft(context.source, receipt)
}
pub fn completion_name_draft(context: &CompletionContext, receipt: &str, name: &str) -> String {
    if !completion_shell_valid(context, receipt) {
        return String::new();
    }
    let report = super::shell_report::receipt_snapshot(context.source, receipt);
    if report.name_catalog
        && report
            .names
            .iter()
            .any(|n| n.name == name && super::shell_resolution::accepts(n.kind))
    {
        name.into()
    } else {
        String::new()
    }
}
fn ranked(
    candidates: impl IntoIterator<Item = CommandSuggestion>,
    query: &str,
    usage: &std::collections::HashMap<String, super::command_history::Usage>,
) -> Vec<CommandSuggestion> {
    use tiderm_core::completion::{rank_indices, CandidateSource, RankCandidate, UsageWeight};
    let candidates: Vec<_> = candidates.into_iter().collect();
    let facts: Vec<_> = candidates
        .iter()
        .map(|item| RankCandidate {
            command: &item.command,
            label: &item.label,
            is_template: item.is_template,
            source: match item.kind {
                0 => CandidateSource::History,
                1 => CandidateSource::User,
                2 => CandidateSource::Syntax,
                3..=6 => CandidateSource::Environment,
                7 => CandidateSource::ExactLookup,
                8..=11 => CandidateSource::Resource,
                12 => CandidateSource::Names,
                _ => CandidateSource::Reference,
            },
            usage: usage
                .get(&item.command)
                .map_or_else(UsageWeight::default, |u| UsageWeight {
                    count: u.count,
                    recent: u.recent,
                    same_directory: u.same_directory,
                }),
        })
        .collect();
    let order = rank_indices(&facts, query);
    let mut candidates: Vec<_> = candidates.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|index| candidates[index].take())
        .collect()
}
#[cfg(test)]
#[path = "command_completion_tests.rs"]
mod tests;
