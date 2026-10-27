//! Source configuration belongs to the session; UI inputs own independent scans.
use super::{command_completion, command_environment, term::try_with_tab};
use crate::bridge::completion::completion_ffi::{
    CompletionContext, CompletionSourceSettings, EnvironmentBatch, ProgramSource,
};

#[derive(Default)]
pub struct Sources {
    settings: CompletionSourceSettings,
    revision: u64,
    terminal_owner: u64,
    terminal_key: Option<(CompletionContext, String)>,
}
impl Sources {
    fn ensure(&mut self, source: u32, generation: u32) {
        if self.settings.source != source
            || self.settings.generation != generation
            || self.revision == 0
        {
            self.cancel();
            self.revision = self.revision.wrapping_add(1).max(1);
            self.settings = CompletionSourceSettings {
                source,
                generation,
                revision: self.revision,
                ..Default::default()
            };
        }
    }
    pub(super) fn cancel(&mut self) {
        if self.terminal_owner != 0 {
            command_environment::environment_close(self.terminal_owner);
        }
        self.terminal_owner = 0;
        self.terminal_key = None;
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub fn completion_sources_revision(source: u32) -> u64 {
    try_with_tab(source, |st| {
        st.completion_sources.ensure(source, st.generation);
        st.completion_sources.revision
    })
    .unwrap_or(0)
}
pub fn completion_sources_get(source: u32) -> CompletionSourceSettings {
    try_with_tab(source, |st| {
        st.completion_sources.ensure(source, st.generation);
        st.completion_sources.settings.clone()
    })
    .unwrap_or_default()
}
pub fn completion_sources_set(
    context: &CompletionContext,
    paths: bool,
    dialect: u8,
    programs_enabled: bool,
    programs: &ProgramSource,
) -> u8 {
    if dialect > 2 {
        return 5;
    }
    if !command_completion::completion_valid(context) {
        return 9;
    }
    if programs_enabled || programs.configured {
        let invalid = super::program_source::programs_validate(context, programs);
        if invalid != 0 {
            return invalid;
        }
    }
    try_with_tab(context.source, |st| {
        st.completion_sources.ensure(context.source, st.generation);
        let current = &st.completion_sources.settings;
        if current.paths == paths
            && current.dialect == dialect
            && current.programs_enabled == programs_enabled
            && current.programs == *programs
        {
            return 0;
        }
        st.completion_sources.cancel();
        st.completion_sources.revision = st.completion_sources.revision.wrapping_add(1).max(1);
        st.completion_sources.settings = CompletionSourceSettings {
            source: context.source,
            generation: st.generation,
            revision: st.completion_sources.revision,
            paths,
            dialect,
            programs_enabled,
            programs: programs.clone(),
        };
        st.input_revision = st.input_revision.wrapping_add(1);
        st.context.shell.clear_request();
        0
    })
    .unwrap_or(9)
}
pub fn terminal_completion_cancel(source: u32) {
    let _ = try_with_tab(source, |st| st.completion_sources.cancel());
}
pub(super) fn terminal_request(
    source: u32,
    query: &str,
    context: &CompletionContext,
) -> (u64, EnvironmentBatch) {
    let state = try_with_tab(source, |st| {
        let sources = &mut st.completion_sources;
        sources.ensure(source, st.generation);
        if !context.ready || (!sources.settings.paths && !sources.settings.programs_enabled) {
            sources.cancel();
            return None;
        }
        if sources.terminal_owner == 0 {
            sources.terminal_owner = command_environment::environment_create();
        }
        let key = (context.clone(), query.to_owned());
        let changed = sources.terminal_key.as_ref() != Some(&key) || sources.terminal_owner == 0;
        sources.terminal_key = Some(key);
        Some((sources.terminal_owner, changed, sources.settings.clone()))
    })
    .flatten();
    let Some((owner, changed, settings)) = state else {
        return (0, EnvironmentBatch::default());
    };
    let result = if changed {
        let mut result = if settings.programs_enabled {
            command_environment::programs_request(
                owner,
                context,
                query,
                settings.dialect,
                &settings.programs,
            )
        } else {
            EnvironmentBatch::default()
        };
        if result.state == 0 && settings.paths {
            result =
                command_environment::environment_request(owner, context, query, settings.dialect);
        }
        result
    } else {
        command_environment::environment_poll(owner)
    };
    (owner, result)
}
#[cfg(test)]
#[path = "completion_sources_tests.rs"]
mod tests;
