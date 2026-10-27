//! Per-terminal sampling. Qt reads cached values; at most one bounded job per pane.
use crate::bridge::context::context_ffi::SystemSnapshot;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiderm_core::system_monitor::{SystemMonitor, SystemSample};

struct State {
    backend: Option<Arc<dyn SystemMonitor>>,
    sample: Option<SystemSample>,
    error: String,
    due: Instant,
    pending: bool,
}

pub struct MonitorPoller {
    state: Arc<Mutex<State>>,
    active: Arc<AtomicBool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Default for MonitorPoller {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                backend: None,
                sample: None,
                error: String::new(),
                due: Instant::now(),
                pending: false,
            })),
            active: Arc::new(AtomicBool::new(true)),
            task: None,
        }
    }
}

impl Drop for MonitorPoller {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl MonitorPoller {
    pub fn snapshot(
        &mut self,
        local: bool,
        protocol: Arc<dyn tiderm_core::protocol::Protocol>,
    ) -> SystemSnapshot {
        let Ok(mut state) = self.state.lock() else {
            return SystemSnapshot {
                enabled: true,
                error: "system monitor state unavailable".into(),
                ..Default::default()
            };
        };
        if !state.pending && Instant::now() >= state.due {
            state.pending = true;
            let backend = state.backend.clone();
            let shared = self.state.clone();
            let active = self.active.clone();
            self.task = Some(super::session::shared_runtime().spawn(async move {
                let backend = match backend {
                    Some(backend) => Ok(backend),
                    None if local => Ok(tiderm_core::system_monitor::local_monitor()),
                    None => protocol.open_system_monitor().await,
                };
                let (backend, result) = match backend {
                    Ok(backend) => {
                        let sample = backend.sample().await;
                        (Some(backend), sample)
                    }
                    Err(error) => (None, Err(error)),
                };
                if !active.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut state) = shared.lock() else {
                    return;
                };
                state.pending = false;
                let succeeded = result.is_ok();
                state.due = Instant::now() + Duration::from_secs(if succeeded { 5 } else { 30 });
                match result {
                    Ok(sample) => {
                        state.backend = backend;
                        state.sample = Some(sample);
                        state.error.clear();
                    }
                    Err(error) => {
                        state.backend = None;
                        state.sample = None;
                        state.error = error.to_string();
                    }
                }
            }));
        }
        let mut snapshot = SystemSnapshot {
            enabled: true,
            error: state.error.clone(),
            ..Default::default()
        };
        if let Some(sample) = &state.sample {
            snapshot.ready = true;
            snapshot.cpu_ready = sample.cpu_percent.is_some();
            snapshot.cpu_percent = f64::from(sample.cpu_percent.unwrap_or_default());
            snapshot.used_bytes = sample.used_bytes;
            snapshot.total_bytes = sample.total_bytes;
        }
        snapshot
    }
}

pub fn context_system_snapshot(idx: u32) -> SystemSnapshot {
    super::term::try_with_tab(idx, |st| {
        use super::store::SessionKind;
        if !st.config.meta.sysmon_enabled || st.manual_stop || !st.host.alive() {
            st.system_monitor = None;
            return SystemSnapshot::default();
        }
        if !matches!(st.config.kind, SessionKind::Local | SessionKind::Ssh) {
            return SystemSnapshot::default();
        }
        match st.host.protocol() {
            Some(protocol) => st
                .system_monitor
                .get_or_insert_with(MonitorPoller::default)
                .snapshot(st.config.kind == SessionKind::Local, protocol),
            None => SystemSnapshot::default(),
        }
    })
    .unwrap_or_default()
}
