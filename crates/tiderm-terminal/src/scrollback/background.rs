//! One lazy process-wide compression worker; bounded batches and explicit read barriers.
use super::{estimate, LineData, ScrollbackStore};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};

const BATCH_BYTES: usize = 512 << 10;
const BATCH_ROWS: usize = 256;
const BACKGROUND_MIN_BYTES: usize = 64 << 10;

struct State {
    store: ScrollbackStore,
    pending: usize,
}
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}
struct Job {
    shared: Arc<Shared>,
    rows: Vec<LineData>,
}

impl Job {
    fn run(self) {
        let mut state = self.shared.state.lock().unwrap();
        for row in self.rows {
            state.store.push(row);
        }
        state.pending -= 1;
        self.shared.ready.notify_all();
    }
}

fn worker() -> Option<&'static mpsc::SyncSender<Job>> {
    static WORKER: OnceLock<Option<mpsc::SyncSender<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            // At most four queued batches plus the active batch, shared by all panes.
            let (tx, rx) = mpsc::sync_channel::<Job>(4);
            std::thread::Builder::new()
                .name("history-compress".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        job.run();
                    }
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()
}

pub(crate) struct BackgroundStore {
    shared: Arc<Shared>,
    budget: usize,
    rows: AtomicUsize,
    bytes: AtomicUsize,
}

impl BackgroundStore {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    store: ScrollbackStore::new(budget),
                    pending: 0,
                }),
                ready: Condvar::new(),
            }),
            budget,
            rows: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        }
    }
    pub(crate) fn push_batch(&mut self, rows: Vec<LineData>) {
        if rows.is_empty() {
            return;
        }
        // The owner is the only producer. Cache exact counts after the next read
        // barrier; repeated viewport projection queries then need no store lock.
        self.rows.store(usize::MAX, Ordering::Relaxed);
        self.bytes.store(usize::MAX, Ordering::Relaxed);
        let mut batch = Vec::new();
        let mut bytes = 0;
        for row in rows {
            bytes += estimate(&row);
            batch.push(row);
            if bytes >= BATCH_BYTES || batch.len() >= BATCH_ROWS {
                self.submit(std::mem::take(&mut batch), bytes);
                bytes = 0;
            }
        }
        if !batch.is_empty() {
            self.submit(batch, bytes);
        }
    }
    fn submit(&self, rows: Vec<LineData>, bytes: usize) {
        if bytes < BACKGROUND_MIN_BYTES {
            // Short batches cost less than a thread handoff. Preserve FIFO after
            // any older background work, then finish this small batch locally.
            let mut state = self.shared.state.lock().unwrap();
            while state.pending != 0 {
                state = self.shared.ready.wait(state).unwrap();
            }
            for row in rows {
                state.store.push(row);
            }
            return;
        }
        self.shared.state.lock().unwrap().pending += 1;
        let job = Job {
            shared: self.shared.clone(),
            rows,
        };
        if let Some(sender) = worker() {
            // Queue pressure waits for the oldest bounded batch; it never drops history
            // or creates unbounded per-pane threads. A failed worker falls back locally.
            if let Err(error) = sender.send(job) {
                error.0.run();
            }
        } else {
            job.run();
        }
    }
    fn read<R>(&self, f: impl FnOnce(&ScrollbackStore) -> R) -> R {
        let mut state = self.shared.state.lock().unwrap();
        while state.pending != 0 {
            state = self.shared.ready.wait(state).unwrap();
        }
        self.rows.store(state.store.rows(), Ordering::Relaxed);
        self.bytes
            .store(state.store.estimated_bytes(), Ordering::Relaxed);
        f(&state.store)
    }
    pub(crate) fn rows(&self) -> usize {
        match self.rows.load(Ordering::Relaxed) {
            usize::MAX => self.read(ScrollbackStore::rows),
            rows => rows,
        }
    }
    pub(crate) fn row(&self, index: usize) -> Option<Arc<LineData>> {
        self.read(|store| store.row(index))
    }
    pub(crate) fn snapshot(&self) -> Vec<Arc<LineData>> {
        self.read(|store| (0..store.rows()).filter_map(|i| store.row(i)).collect())
    }
    pub(crate) fn estimated_bytes(&self) -> usize {
        match self.bytes.load(Ordering::Relaxed) {
            usize::MAX => self.read(ScrollbackStore::estimated_bytes),
            bytes => bytes,
        }
    }
    pub(crate) fn budget(&self) -> usize {
        self.budget
    }
    pub(crate) fn clear(&mut self) {
        // Queued work owns the old generation. It can never repopulate a cleared pane.
        *self = Self::new(self.budget);
    }
}
