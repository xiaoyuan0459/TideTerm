//! One IO worker; each browser retains at most one pending job and one snapshot.
//! Requests replace earlier jobs. Close/cancel prevents stale results becoming visible.
use crate::bridge::files::files_ffi::{LocalEntry, LocalSnapshot};
use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex, OnceLock};

#[derive(Default)]
struct Browser {
    revision: u64,
    pending: Option<String>,
    result: Option<LocalSnapshot>,
}
#[derive(Default)]
struct State {
    next: u64,
    browsers: BTreeMap<u64, Browser>,
}
struct Worker {
    state: Mutex<State>,
    wake: Condvar,
}

fn worker() -> &'static Worker {
    static WORKER: OnceLock<Worker> = OnceLock::new();
    let worker = WORKER.get_or_init(|| Worker {
        state: Mutex::new(State::default()),
        wake: Condvar::new(),
    });
    static THREAD: OnceLock<()> = OnceLock::new();
    THREAD.get_or_init(|| {
        std::thread::Builder::new()
            .name("local-files".into())
            .spawn(move || work(worker))
            .expect("local files worker");
    });
    worker
}

fn empty() -> LocalSnapshot {
    LocalSnapshot {
        ready: false,
        path: String::new(),
        error: String::new(),
        truncated: false,
        entries: Vec::new(),
    }
}
pub fn files_create() -> u64 {
    let mut state = worker().state.lock().unwrap();
    if state.browsers.len() >= 16 {
        return 0;
    }
    state.next += 1;
    let id = state.next;
    state.browsers.insert(id, Browser::default());
    id
}
pub fn files_close(owner: u64) {
    worker().state.lock().unwrap().browsers.remove(&owner);
    super::file_jobs::close(owner);
}
pub(super) fn files_exists(owner: u64) -> bool {
    worker().state.lock().unwrap().browsers.contains_key(&owner)
}
pub fn files_request(owner: u64, path: &str) {
    if let Some(browser) = worker().state.lock().unwrap().browsers.get_mut(&owner) {
        browser.revision += 1;
        browser.pending = Some(path.to_owned());
        browser.result = None;
        worker().wake.notify_one();
    }
}
pub fn files_poll(owner: u64) -> LocalSnapshot {
    worker()
        .state
        .lock()
        .unwrap()
        .browsers
        .get_mut(&owner)
        .and_then(|b| b.result.take())
        .unwrap_or_else(empty)
}

fn work(worker: &'static Worker) {
    loop {
        let (owner, revision, path) = {
            let mut state = worker.state.lock().unwrap();
            loop {
                if let Some(job) = state.browsers.iter_mut().find_map(|(id, browser)| {
                    browser
                        .pending
                        .take()
                        .map(|path| (*id, browser.revision, path))
                }) {
                    break job;
                }
                state = worker.wake.wait(state).unwrap();
            }
        };
        let current = || {
            worker
                .state
                .lock()
                .unwrap()
                .browsers
                .get(&owner)
                .is_some_and(|b| b.revision == revision)
        };
        let mut result = empty();
        result.ready = true;
        result.path = path.clone();
        match std::fs::read_dir(&path) {
            Err(e) => result.error = e.to_string(),
            Ok(entries) => {
                for entry in entries {
                    if !current() {
                        break;
                    }
                    if result.entries.len() == 10_000 {
                        result.truncated = true;
                        break;
                    }
                    let entry = match entry {
                        Ok(e) => e,
                        Err(e) => {
                            result.error = e.to_string();
                            break;
                        }
                    };
                    match entry.metadata() {
                        Ok(meta) => result.entries.push(LocalEntry {
                            name: entry.file_name().to_string_lossy().into_owned(),
                            is_dir: meta.is_dir()
                                || (meta.file_type().is_symlink() && entry.path().is_dir()),
                            size: meta.len(),
                            modified: meta
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0),
                        }),
                        Err(e) => {
                            result.error = e.to_string();
                        }
                    }
                }
                result
                    .entries
                    .sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
            }
        }
        let mut state = worker.state.lock().unwrap();
        if let Some(browser) = state
            .browsers
            .get_mut(&owner)
            .filter(|b| b.revision == revision)
        {
            browser.result = Some(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Contract: a replaced request cannot overwrite the visible directory, and
    // missing directories produce an error instead of looking like an empty folder.
    #[test]
    fn replacement_and_directory_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), b"hello").unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        let owner = files_create();
        let wait = || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                let result = files_poll(owner);
                if result.ready {
                    break result;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "directory scan timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        files_request(owner, &dir.path().join("missing").to_string_lossy());
        files_request(owner, &dir.path().to_string_lossy());
        let result = wait();
        assert!(result.error.is_empty(), "{}", result.error);
        assert_eq!(result.entries.len(), 2);
        assert!(result.entries[0].is_dir);
        assert_eq!(result.entries[1].size, 5);
        files_request(owner, &dir.path().join("missing").to_string_lossy());
        assert!(!wait().error.is_empty());
        files_close(owner);
    }
}
