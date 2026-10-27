//! Session-scoped callback ownership and bounded incoming forward channels.
use super::{raw_channel::Channel, x11::X11};
use crate::{
    error::{Error, Result},
    session_options::SshOptions,
};
use libssh2_sys as raw;
use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{mpsc, watch};

unsafe extern "C" {
    fn libssh2_session_callback_set(
        session: *mut raw::LIBSSH2_SESSION,
        kind: i32,
        callback: *mut c_void,
    ) -> *mut c_void;
}
struct Pending {
    sender: mpsc::Sender<(usize, bool)>,
}
fn registry() -> &'static Mutex<HashMap<usize, Weak<Pending>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, Weak<Pending>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub(super) struct Forwarding {
    cancel: watch::Sender<bool>,
}
impl Forwarding {
    pub fn stop(&self) {
        let _ = self.cancel.send(true);
    }
}
impl Drop for Forwarding {
    fn drop(&mut self) {
        self.stop();
    }
}
fn accept(session: *mut raw::LIBSSH2_SESSION, channel: *mut raw::LIBSSH2_CHANNEL, x11: bool) {
    // Callback already holds the ssh2 session lock. Only queue ownership here;
    // local I/O and subsequent channel operations run outside that lock.
    let pending = registry()
        .lock()
        .ok()
        .and_then(|r| r.get(&(session as usize)).and_then(Weak::upgrade));
    if pending.is_none_or(|p| p.sender.try_send((channel as usize, x11)).is_err()) {
        unsafe {
            raw::libssh2_channel_free(channel);
        }
    }
}
unsafe extern "C" fn agent(
    session: *mut raw::LIBSSH2_SESSION,
    channel: *mut raw::LIBSSH2_CHANNEL,
    _: *mut *mut c_void,
) {
    accept(session, channel, false);
}
unsafe extern "C" fn x11(
    session: *mut raw::LIBSSH2_SESSION,
    channel: *mut raw::LIBSSH2_CHANNEL,
    _: *const std::ffi::c_char,
    _: i32,
    _: *mut *mut c_void,
) {
    accept(session, channel, true);
}

impl Forwarding {
    pub fn start(session: &ssh2::Session, options: &SshOptions) -> Result<(Self, Option<X11>)> {
        let xconfig = if options.x11_enabled {
            Some(X11::load(options)?)
        } else {
            None
        };
        let agent_path = if options.agent_forwarding {
            Some(super::forward_relay::agent_path()?)
        } else {
            None
        };
        let runtime = tokio::runtime::Handle::current();
        if let Some(path) = &agent_path {
            runtime.block_on(super::forward_relay::agent_connect(path))?;
        }
        let (sender, mut receiver) = mpsc::channel(16);
        let pending = Arc::new(Pending { sender });
        let (cancel, mut cancelled) = watch::channel(false);
        let key = {
            let mut guard = session.raw();
            let ptr = &mut *guard as *mut raw::LIBSSH2_SESSION;
            registry()
                .lock()
                .map_err(|_| Error::Ssh("forwarding registry poisoned".into()))?
                .insert(ptr as usize, Arc::downgrade(&pending));
            unsafe {
                if options.agent_forwarding {
                    libssh2_session_callback_set(ptr, 7, agent as *const () as *mut _);
                }
                if options.x11_enabled {
                    libssh2_session_callback_set(ptr, 4, x11 as *const () as *mut _);
                }
            }
            ptr as usize
        };
        let session = session.clone();
        let xconfig_task = xconfig.clone();
        runtime.spawn(async move {
            let _pending = pending;
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancelled.changed() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    Some((ptr, is_x11)) = receiver.recv() => {
                        let channel = unsafe { Channel::accepted(ptr, &session) };
                        if tasks.len() >= 32 { drop(channel); continue; }
                        let xconfig = xconfig_task.clone();
                        let path = agent_path.clone();
                        tasks.spawn(async move {
                            let result = super::forward_relay::relay(channel, if is_x11 { xconfig } else { None }, if is_x11 { None } else { path }).await;
                            if let Err(error) = result {
                                tracing::warn!(target: "ssh", "forward channel: {error}");
                            }
                        });
                    }
                }
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            let _guard = session.raw();
            let ptr = key as *mut raw::LIBSSH2_SESSION;
            unsafe {
                libssh2_session_callback_set(ptr, 7, std::ptr::null_mut());
                libssh2_session_callback_set(ptr, 4, std::ptr::null_mut());
            }
            if let Ok(mut registry) = registry().lock() { registry.remove(&key); }
            drop(_guard);
            receiver.close();
            while let Ok((ptr, _)) = receiver.try_recv() { drop(unsafe { Channel::accepted(ptr, &session) }); }
        });
        Ok((Self { cancel }, xconfig))
    }
}
