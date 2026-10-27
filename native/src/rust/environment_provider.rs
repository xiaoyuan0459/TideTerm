use super::{environment_paths, environment_remote, store::SessionKind, term::try_with_tab};
use crate::bridge::completion::completion_ffi::EnvironmentBatch;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;
use tiderm_core::protocol::Protocol;

pub(super) enum Provider {
    Local(environment_paths::Plan),
    Ssh(environment_remote::Plan, Arc<dyn Protocol>),
    Programs(super::program_source::Plan),
}
pub(super) fn eligible(source: u32) -> bool {
    try_with_tab(source, |st| {
        st.config.kind == SessionKind::Local
            || (st.config.kind == SessionKind::Ssh && st.config.meta.sftp_enabled)
    })
    .unwrap_or(false)
}
impl Provider {
    pub(super) fn capture(source: u32, cwd: &str, query: &str, dialect: u8) -> Result<Self, u8> {
        let (kind, reported, allowed, protocol) = try_with_tab(source, |st| {
            (
                st.config.kind,
                st.context.cwd_reported,
                st.config.meta.sftp_enabled,
                st.host.protocol(),
            )
        })
        .ok_or(9)?;
        if !matches!(kind, SessionKind::Local | SessionKind::Ssh) {
            return Err(3);
        }
        if kind == SessionKind::Ssh && !allowed {
            return Err(11);
        }
        if !reported || cwd.is_empty() {
            return Err(4);
        }
        if kind == SessionKind::Local {
            return environment_paths::plan(cwd, query, dialect)
                .map(Self::Local)
                .ok_or(5);
        }
        Ok(Self::Ssh(
            environment_remote::plan(cwd, query, dialect).ok_or(5)?,
            protocol.ok_or(9)?,
        ))
    }
    pub(super) fn remote(&self) -> bool {
        match self {
            Self::Ssh(..) => true,
            Self::Programs(plan) => plan.remote(),
            _ => false,
        }
    }
    pub(super) fn scan(&self, deadline: Instant, canceled: Arc<AtomicBool>) -> EnvironmentBatch {
        match self {
            Self::Local(plan) => environment_paths::scan(plan, deadline, || {
                !canceled.load(std::sync::atomic::Ordering::Relaxed)
            }),
            Self::Ssh(plan, protocol) => {
                environment_remote::scan(plan, protocol, deadline, canceled)
            }
            Self::Programs(plan) => plan.scan(deadline, canceled),
        }
    }
}
