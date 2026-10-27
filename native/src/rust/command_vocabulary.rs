//! Convert shared reference facts to the Qt bridge; no discovery or execution.
use crate::bridge::completion::completion_ffi::{CommandSuggestion, CompletionContext};
pub(super) fn suggestions(context: &CompletionContext, query: &str) -> Vec<CommandSuggestion> {
    tiderm_core::completion::reference_suggestions(
        &tiderm_core::completion::VocabularyContext {
            shell_kind: context.shell_kind,
            system: context.system,
        },
        query,
    )
    .into_iter()
    .map(|item| CommandSuggestion {
        kind: 13,
        command: item.command,
        label: item.label,
        description: item.description,
        detail: item.detail,
        reference: item.reference,
        ..Default::default()
    })
    .collect()
}
