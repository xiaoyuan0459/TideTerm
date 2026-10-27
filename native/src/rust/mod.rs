//! 引擎宿主模块（v1 §5 的 `rust/` 位）：终端面（term）+ 会话面（session）。
//!
//! 数据流：`cpp/` 视图 → 桥（`ffi.rs`）→ 本模块 → tiderm-terminal 引擎；回程像素经
//! `PaneSurface::as_raw()` 零拷贝过桥。C++ 侧看不到本模块的任何 trait（铁律 2）。

pub mod ai;
pub mod alloc_probe;
pub mod command_completion;
pub mod command_environment;
pub mod command_history;
pub mod command_library;
pub mod command_rules;
pub mod command_template;
mod command_vocabulary;
mod completion_shell;
pub mod completion_sources;
mod continuous_log;
mod docker_snapshot;
mod environment_draft;
mod environment_paths;
mod environment_provider;
mod environment_remote;
pub mod events;
mod file_endpoint;
pub mod file_jobs;
#[cfg(test)]
mod file_jobs_remote_tests;
mod file_tree;
mod git_refs;
pub mod history_export;
mod history_file;
pub mod history_pager;
mod kube_snapshot;
mod local_cwd;
pub mod local_files;
pub mod local_shell;
pub mod log;
pub mod log_settings;
pub mod mark_persistence;
mod mark_profile;
pub mod modem;
#[cfg(test)]
mod modem_tests;
pub mod multiline;
#[cfg(test)]
mod multiline_tests;
pub mod onekey;
pub mod palette;
#[cfg(test)]
mod palette_tests;
pub mod pod;
mod program_scan;
pub mod program_source;
pub mod quick_target;
pub mod remote_edit;
pub mod screen_lock;
mod send_text;
pub mod session;
pub mod session_advanced;
mod session_codec;
pub mod session_defaults;
pub mod session_explorer;
mod session_form_config;
mod session_input;
pub mod session_io;
mod session_modem;
#[cfg(test)]
mod session_options_tests;
pub mod sftp;
mod shell_names;
pub mod shell_report;
mod shell_report_wire;
mod shell_resolution;
pub mod spike;
pub mod store;
pub mod sync_channels;
#[cfg(test)]
mod sync_channels_tests;
pub mod system_monitor;
pub mod term;
pub mod term_actions;
pub mod term_annotations;
pub mod term_commands;
pub mod term_context;
pub mod term_create;
pub mod term_diag;
#[cfg(test)]
mod term_esc_tests;
pub mod term_folds;
mod term_font;
pub mod term_highlight;
pub mod term_history;
mod term_input;
#[cfg(test)]
mod term_input_tests;
pub mod term_keys;
pub mod term_lifecycle;
pub mod term_log;
pub mod term_marks;
#[cfg(test)]
mod term_marks_tests;
mod term_modes;
#[cfg(test)]
mod term_net_tests;
pub mod term_ops;
mod term_pointer;
pub mod term_reading;
#[cfg(test)]
mod term_reading_tests;
pub mod term_selection;
#[cfg(test)]
mod term_seq_tests;
#[cfg(test)]
mod term_ssh_tests;
#[cfg(test)]
mod term_tab_tests;
#[cfg(test)]
mod term_tests;
pub mod terminal_completion;
#[cfg(test)]
mod terminal_completion_tests;
mod terminal_host;
pub mod tmux_session;
pub mod vault;
pub mod workspace;

// 主题服务（⑦）实体挂在 crate 根 `crate::theme`（main.rs #[path] 挂载，满足
// theme_db/ui_prefs 复用件与 prefs 桥的 `crate::theme::…` 引用），不占本模块。
