pub use crate::rust::command_completion::{
    completion_context, completion_docker_draft, completion_environment_query,
    completion_git_draft, completion_kube_draft, completion_name_draft, completion_query,
    completion_shell_draft, completion_shell_valid, completion_valid,
};
pub use crate::rust::command_environment::{
    environment_cancel, environment_close, environment_create, environment_poll,
    environment_request, environment_valid, programs_request,
};
pub use crate::rust::command_library::{command_catalog, command_catalog_save};
pub use crate::rust::command_template::{command_template_fields, command_template_render};
pub use crate::rust::completion_sources::{
    completion_sources_get, completion_sources_revision, completion_sources_set,
    terminal_completion_cancel,
};
pub use crate::rust::program_source::programs_validate;
pub use crate::rust::shell_report::{
    shell_docker_prepare, shell_git_prepare, shell_kube_prepare, shell_kube_send,
    shell_names_prepare, shell_report_cancel, shell_report_clear, shell_report_poll,
    shell_report_prepare, shell_report_send, shell_resolution_draft, shell_resolution_prepare,
};
pub use crate::rust::terminal_completion::{terminal_completion_accept, terminal_completion_query};

#[cxx::bridge]
pub mod completion_ffi {
    #[derive(Clone, Default)]
    pub struct CommandName {
        pub name: String,
        pub kind: u8,
    }
    #[derive(Clone, Default, PartialEq, Eq)]
    pub struct KubeScope {
        /// 0=Pod, 1=Service, 2=Deployment; part of the authenticated snapshot scope.
        pub resource: u8,
        pub config: String,
        pub context: String,
        pub namespace_name: String,
    }
    #[derive(Clone, Default)]
    pub struct KubeEntry {
        pub uid: String,
        pub name: String,
    }
    #[derive(Clone, Default)]
    pub struct ContainerEntry {
        pub id: String,
        pub name: String,
    }
    #[derive(Clone, Default)]
    pub struct ShellReport {
        pub system: u8,
        pub name_catalog: bool,
        pub name_prefix: String,
        pub name_error: u8,
        pub names: Vec<CommandName>,
        pub source: u32,
        /// 0=off, 1=preview, 2=waiting, 3=reported, 4=timeout, 5=syntax,
        /// 6=source unavailable, 7=expired, 8=send rejected, 9=invalid exact name, 10=Git failure/budget.
        /// 11=Docker failure/budget, 12=invalid explicit Docker context,
        /// 13=invalid kube scope, 14=kubectl failure/budget, 15=credential confirmation required.
        pub state: u8,
        /// 1=Bash, 2=PowerShell Windows, 3=PowerShell Unix. Shell-reported.
        pub platform: u8,
        pub age_seconds: u32,
        pub token: String,
        pub command: String,
        pub path: String,
        /// Empty for PATH/Git/Docker/kubectl, otherwise an exact ASCII command name.
        pub query: String,
        /// 0=missing, 1=alias, 2=function, 3=builtin, 4=keyword, 5=application,
        /// 6=external script, 7=cmdlet, 8=filter, 9=configuration (display only).
        pub kind: u8,
        pub location: String,
        pub can_fill: bool,
        /// Explicit Git ref snapshot; shares the one-shot report channel.
        pub git: bool,
        pub git_error: u8,
        pub repository: String,
        pub references: Vec<String>,
        pub truncated: bool,
        pub docker_context: String,
        pub docker_error: u8,
        pub containers: Vec<ContainerEntry>,
        pub kube: KubeScope,
        pub kube_error: u8,
        pub resources: Vec<KubeEntry>,
    }
    #[derive(Clone, Default, PartialEq, Eq)]
    pub struct UserCommand {
        pub id: u32,
        pub group: String,
        pub name: String,
        pub command: String,
        pub is_template: bool,
    }
    pub struct CommandCatalog {
        pub revision: u64,
        pub entries: Vec<UserCommand>,
        /// 0=ok, 1=read/format, 2=invalid/limit, 3=conflict, 4=write, 5=locked.
        pub error: u8,
    }
    #[derive(Clone, Default, PartialEq, Eq)]
    pub struct CompletionContext {
        /// 0=unknown, 1=Bash, 2=PowerShell, 3=cmd, 4=zsh, 5=fish, 6=POSIX sh.
        pub shell_kind: u8,
        /// 0=unknown, 1=Windows, 2=Linux, 3=macOS. Remote is never inferred from local OS.
        pub system: u8,
        pub source: u32,
        pub generation: u32,
        pub input_revision: u64,
        pub history_revision: u64,
        pub catalog_revision: u64,
        pub cwd: String,
        pub directory_revision: u64,
        pub sftp_enabled: bool,
        pub ready: bool,
    }
    #[derive(Clone, Default)]
    pub struct CommandSuggestion {
        /// 0=history, 1=user, 2=rule, 3=Local path, 4=SSH path, 5=Local program directory,
        /// 6=SSH program directory, 7=explicit Shell resolution, 8=Git branch, 9=Git tag, 10=Docker, 11=Pod.
        pub kind: u8,
        pub label: String,
        pub command: String,
        pub user_id: u32,
        pub is_template: bool,
        pub description: String,
        pub detail: String,
        pub reference: String,
    }
    pub struct CompletionBatch {
        pub context: CompletionContext,
        pub items: Vec<CommandSuggestion>,
        pub rule_error: bool,
        /// Matching explicit name/Git snapshot; fixed probe command and PATH are omitted.
        pub shell: ShellReport,
    }
    pub struct TerminalCompletion {
        pub environment_owner: u64,
        pub environment_request: u64,
        pub environment_state: u8,
        pub environment_truncated: bool,
        pub environment_skipped: bool,
        pub batch: CompletionBatch,
        pub query: String,
        pub output_revision: u64,
    }
    #[derive(Clone, Default)]
    pub struct EnvironmentBatch {
        pub request: u64,
        /// 0=idle, 1=pending, 2=ready, 3=transport, 4=directory, 5=syntax,
        /// 6=IO, 7=timeout, 8=capacity/worker, 9=stale, 10=remote service, 11=SFTP disabled.
        /// 12=program directories missing, 13=invalid directories, 14=configuration source changed.
        pub state: u8,
        pub truncated: bool,
        pub skipped: bool,
        pub items: Vec<CommandSuggestion>,
    }
    #[derive(Clone, Default, PartialEq, Eq)]
    pub struct ProgramSource {
        pub configured: bool,
        pub source: u32,
        pub generation: u32,
        pub directories: Vec<String>,
    }
    #[derive(Clone, Default, PartialEq, Eq)]
    pub struct CompletionSourceSettings {
        pub source: u32,
        pub generation: u32,
        pub revision: u64,
        pub paths: bool,
        pub dialect: u8,
        pub programs_enabled: bool,
        pub programs: ProgramSource,
    }
    pub struct TemplateFields {
        pub names: Vec<String>,
        pub error: u8,
    }
    pub struct TemplatePreview {
        pub text: String,
        /// 0=ok, 1=stale, 2=missing entry, 3=syntax, 4=value/budget.
        pub error: u8,
    }
    extern "Rust" {
        fn completion_sources_get(source: u32) -> CompletionSourceSettings;
        fn completion_sources_revision(source: u32) -> u64;
        fn completion_sources_set(
            context: &CompletionContext,
            paths: bool,
            dialect: u8,
            programs_enabled: bool,
            programs: &ProgramSource,
        ) -> u8;
        fn terminal_completion_cancel(source: u32);
        fn completion_name_draft(context: &CompletionContext, receipt: &str, name: &str) -> String;
        fn shell_report_prepare(source: u32, dialect: u8) -> ShellReport;
        fn shell_names_prepare(source: u32, dialect: u8, prefix: &str) -> ShellReport;
        fn shell_report_send(source: u32, token: &str) -> u8;
        fn shell_report_poll(source: u32) -> ShellReport;
        fn shell_report_clear(source: u32);
        fn shell_report_cancel(source: u32);
        fn shell_resolution_prepare(source: u32, dialect: u8, name: &str) -> ShellReport;
        fn shell_resolution_draft(source: u32, receipt: &str) -> String;
        fn shell_git_prepare(source: u32, dialect: u8) -> ShellReport;
        fn shell_docker_prepare(source: u32, dialect: u8, context: &str) -> ShellReport;
        fn shell_kube_prepare(source: u32, dialect: u8, scope: &KubeScope) -> ShellReport;
        fn shell_kube_send(source: u32, token: &str, credentials_confirmed: bool) -> u8;
        fn completion_kube_draft(
            context: &CompletionContext,
            receipt: &str,
            query: &str,
            uid: &str,
        ) -> String;
        fn completion_docker_draft(
            context: &CompletionContext,
            receipt: &str,
            query: &str,
            id: &str,
        ) -> String;
        fn completion_git_draft(
            context: &CompletionContext,
            receipt: &str,
            query: &str,
            reference: &str,
        ) -> String;
        fn command_catalog() -> CommandCatalog;
        fn command_catalog_save(revision: u64, entries: &[UserCommand]) -> u8;
        fn completion_query(source: u32, query: &str) -> CompletionBatch;
        fn terminal_completion_query(source: u32) -> TerminalCompletion;
        fn terminal_completion_accept(snapshot: &TerminalCompletion, index: u32) -> bool;
        fn completion_environment_query(source: u32, query: &str, owner: u64) -> CompletionBatch;
        fn environment_create() -> u64;
        fn environment_close(owner: u64);
        fn environment_cancel(owner: u64);
        fn environment_request(
            owner: u64,
            context: &CompletionContext,
            query: &str,
            dialect: u8,
        ) -> EnvironmentBatch;
        fn environment_poll(owner: u64) -> EnvironmentBatch;
        fn programs_validate(context: &CompletionContext, settings: &ProgramSource) -> u8;
        fn programs_request(
            owner: u64,
            context: &CompletionContext,
            query: &str,
            dialect: u8,
            settings: &ProgramSource,
        ) -> EnvironmentBatch;
        fn environment_valid(
            owner: u64,
            request: u64,
            context: &CompletionContext,
            query: &str,
        ) -> bool;
        fn completion_context(source: u32) -> CompletionContext;
        fn completion_valid(context: &CompletionContext) -> bool;
        fn completion_shell_valid(context: &CompletionContext, receipt: &str) -> bool;
        fn completion_shell_draft(context: &CompletionContext, receipt: &str) -> String;
        fn command_template_fields(text: &str) -> TemplateFields;
        fn command_template_render(
            context: &CompletionContext,
            id: u32,
            values: &[String],
        ) -> TemplatePreview;
    }
}
