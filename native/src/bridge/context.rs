//! Focused terminal context and validated address targets.
pub use crate::rust::command_history::{history_clear, history_commands, history_revision};
pub use crate::rust::quick_target::{context_open_shell, context_parse_address};
pub use crate::rust::session_advanced::{context_right_click_paste, context_take_bell};
pub use crate::rust::system_monitor::context_system_snapshot;
pub use crate::rust::term_annotations::context_line_times;
pub use crate::rust::term_context::{term_current_dir, term_split_tab};

#[cxx::bridge]
pub mod context_ffi {
    #[derive(Default)]
    pub struct SystemSnapshot {
        pub enabled: bool,
        pub ready: bool,
        pub cpu_ready: bool,
        pub cpu_percent: f64,
        pub used_bytes: u64,
        pub total_bytes: u64,
        pub error: String,
    }
    pub struct LineTime {
        pub baseline: f32,
        pub stamp: String,
    }
    pub struct QuickTarget {
        pub ok: bool,
        pub kind: u8,
        pub host: String,
        pub port: u32,
        pub user: String,
        pub error: String,
    }
    #[derive(Default)]
    pub struct HostPreview {
        pub ready: bool,
        pub banner: String,
        pub fingerprint: String,
        /// 0=pending/error, 1=known, 2=unknown, 3=changed.
        pub state: u8,
        pub error: String,
    }
    extern "Rust" {
        fn context_take_bell(idx: u32) -> bool;
        fn context_right_click_paste(idx: u32) -> bool;
        fn context_system_snapshot(idx: u32) -> SystemSnapshot;
        fn history_revision(idx: u32) -> u64;
        fn history_commands(idx: u32) -> Vec<String>;
        fn history_clear(idx: u32);
        fn term_current_dir(idx: u32) -> String;
        fn term_split_tab(idx: u32) -> u32;
        fn context_line_times(idx: u32) -> Vec<LineTime>;
        fn context_parse_address(address: &str) -> QuickTarget;
        fn context_open_shell(shell: &str) -> u32;
        fn context_shell_display_name(shell: &str) -> String;
        fn context_inspect_host(host: &str, port: u32) -> u64;
        fn context_take_host_preview(request: u64) -> HostPreview;
        fn context_cancel_host_preview(request: u64);
    }
}

use std::sync::{Mutex, OnceLock};

fn context_shell_display_name(shell: &str) -> String {
    crate::rust::store::session_config::shell_display_name(shell)
}

#[derive(Default)]
struct PreviewState {
    request: u64,
    reply: Option<context_ffi::HostPreview>,
}

fn preview_state() -> &'static Mutex<PreviewState> {
    static STATE: OnceLock<Mutex<PreviewState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PreviewState::default()))
}

fn context_inspect_host(host: &str, port: u32) -> u64 {
    let request = {
        let mut state = preview_state().lock().unwrap();
        state.request = state.request.wrapping_add(1);
        state.reply = None;
        state.request
    };
    let host = host.to_owned();
    crate::rust::session::shared_runtime().spawn(async move {
        use tiderm_core::known_hosts::KnownHostStatus;
        let result = tiderm_core::protocol::ssh::inspect::inspect_host(host, port as u16, 5).await;
        let reply = match result {
            Ok(info) => context_ffi::HostPreview {
                ready: true,
                banner: info.banner,
                fingerprint: info.fingerprint,
                state: match info.status {
                    KnownHostStatus::Known => 1,
                    KnownHostStatus::Unknown => 2,
                    KnownHostStatus::KeyChanged => 3,
                },
                error: String::new(),
            },
            Err(error) => context_ffi::HostPreview {
                ready: true,
                error: error.to_string(),
                ..Default::default()
            },
        };
        let mut state = preview_state().lock().unwrap();
        if state.request == request {
            state.reply = Some(reply);
        }
    });
    request
}

fn context_take_host_preview(request: u64) -> context_ffi::HostPreview {
    let mut state = preview_state().lock().unwrap();
    if state.request == request {
        state.reply.take().unwrap_or_default()
    } else {
        Default::default()
    }
}

fn context_cancel_host_preview(request: u64) {
    let mut state = preview_state().lock().unwrap();
    if state.request == request {
        state.request = state.request.wrapping_add(1);
        state.reply = None;
    }
    // Do not abort the bounded handshake: it must shut down its blocking socket.
}
