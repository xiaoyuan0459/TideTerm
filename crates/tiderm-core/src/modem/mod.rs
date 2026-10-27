//! Terminal-stream transfers. One session owns one transfer and one output route.
mod files;
#[cfg(test)]
mod tests;
mod wire;
mod xy;
mod z;
use crate::protocol::Protocol;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::{mpsc, Notify};

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    X,
    Y,
    Z,
}
pub enum Request {
    Send { kind: Kind, paths: Vec<PathBuf> },
    Receive { kind: Kind, path: PathBuf },
}
#[derive(Clone, Default)]
pub struct Progress {
    pub bytes: u64,
    pub file: String,
    pub finished: bool,
    pub error: String,
}
struct State {
    busy: AtomicBool,
    cancelled: AtomicBool,
    cancel: Notify,
    route: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    progress: Mutex<Progress>,
    output: Mutex<Vec<u8>>,
}
#[derive(Clone)]
pub struct Router(Arc<State>);
impl Default for Router {
    fn default() -> Self {
        Self(Arc::new(State {
            busy: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            cancel: Notify::new(),
            route: Mutex::new(None),
            progress: Mutex::new(Progress::default()),
            output: Mutex::new(Vec::new()),
        }))
    }
}
impl Router {
    pub fn busy(&self) -> bool {
        self.0.busy.load(Ordering::SeqCst)
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.cancel.notify_waiters();
    }
    pub fn eof(&self) {
        self.0
            .route
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }
    pub fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.output.lock().unwrap_or_else(|p| p.into_inner()))
    }
    pub fn progress(&self) -> Progress {
        self.0
            .progress
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn reserve(&self, request: Request, timeout_secs: u32) -> Result<Job, String> {
        if !(1..=120).contains(&timeout_secs) {
            return Err("invalid Modem timeout".into());
        }
        if self
            .0
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("a transfer is already running".into());
        }
        self.0.cancelled.store(false, Ordering::SeqCst);
        *self.0.progress.lock().unwrap_or_else(|p| p.into_inner()) = Progress::default();
        let (tx, rx) = mpsc::channel(8);
        *self.0.route.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        Ok(Job {
            router: self.clone(),
            request,
            timeout_secs,
            receiver: tokio::sync::Mutex::new(Some(rx)),
        })
    }
    /// Called by the existing sole output reader before charset/OSC/log processing.
    pub async fn route(&self, bytes: Vec<u8>) -> Option<Vec<u8>> {
        let sender = self
            .0
            .route
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(sender) = sender {
            match sender.send(bytes).await {
                Ok(()) => None,
                Err(error) => Some(self.terminal_output(error.0)),
            }
        } else {
            Some(self.terminal_output(bytes))
        }
    }
    fn terminal_output(&self, bytes: Vec<u8>) -> Vec<u8> {
        // Completion publishes the retained tail before releasing the route.
        // This also preserves order when a reader was waiting on a full queue.
        let _route = self.0.route.lock().unwrap_or_else(|p| p.into_inner());
        let mut output = self.take_output();
        output.extend(bytes);
        output
    }
    fn update(&self, bytes: u64, file: &str) {
        let mut progress = self.0.progress.lock().unwrap_or_else(|p| p.into_inner());
        progress.bytes = bytes;
        progress.file = file.into();
    }
}
pub struct Job {
    router: Router,
    request: Request,
    timeout_secs: u32,
    receiver: tokio::sync::Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
}
impl Job {
    pub async fn run(&self, protocol: Arc<dyn Protocol>) {
        let Some(rx) = self.receiver.lock().await.take() else {
            return;
        };
        let mut wire = wire::Wire::new(protocol, rx, self.router.clone(), self.timeout_secs);
        let result = match &self.request {
            Request::Send { kind, paths } => match kind {
                Kind::X | Kind::Y => xy::send(&mut wire, *kind, paths).await,
                Kind::Z => z::send(&mut wire, paths).await,
            },
            Request::Receive { kind, path } => match kind {
                Kind::X | Kind::Y => xy::receive(&mut wire, *kind, path).await,
                Kind::Z => z::receive(&mut wire, path).await,
            },
        };
        if result.is_err() {
            let _ = wire.abort().await;
        } else {
            let mut route = self
                .router
                .0
                .route
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            self.router
                .0
                .output
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .extend(wire.remainder());
            route.take();
        }
        let mut progress = self
            .router
            .0
            .progress
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        progress.finished = true;
        progress.error = result.err().unwrap_or_default();
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.router
            .0
            .route
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let mut p = self
            .router
            .0
            .progress
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !p.finished {
            p.finished = true;
            p.error = "transfer cancelled".into();
        }
        drop(p);
        // Publish availability only after all state belonging to this job is
        // finalized; a new reservation must not inherit a late completion write.
        self.router.0.busy.store(false, Ordering::SeqCst);
    }
}
