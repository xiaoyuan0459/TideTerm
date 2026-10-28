//! O(1) lookup for large transfer queues. Call only while holding the queue lock.

use super::sftp_model::{TransferDirection, TransferItem};
use super::transfer_limit;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, PartialEq, Eq, Hash)]
enum Target {
    Upload(String, String),
    Download(PathBuf),
}

#[derive(Default)]
struct Index {
    by_id: HashMap<String, usize>,
    active_targets: HashMap<Target, (String, u32)>,
    targets_by_id: HashMap<String, Target>,
    counts: HashMap<String, [usize; 4]>,
}

fn index() -> &'static Mutex<Index> {
    static INDEX: OnceLock<Mutex<Index>> = OnceLock::new();
    INDEX.get_or_init(|| Mutex::new(Index::default()))
}

fn target(item: &TransferItem) -> Target {
    match item.direction {
        TransferDirection::Upload => {
            Target::Upload(item.session_id.clone(), item.remote_path.clone())
        }
        TransferDirection::Download => {
            let absolute = std::path::absolute(&item.local_path)
                .unwrap_or_else(|_| PathBuf::from(&item.local_path));
            let path = absolute
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .zip(absolute.file_name())
                .map_or_else(|| absolute.clone(), |(parent, name)| parent.join(name));
            #[cfg(windows)]
            let path = PathBuf::from(path.to_string_lossy().to_lowercase());
            Target::Download(path)
        }
    }
}

pub(super) fn reserve(item: &TransferItem) -> bool {
    let mut index = index().lock().unwrap();
    let weight = transfer_limit::weight(item.total_bytes);
    let target = target(item);
    if index.active_targets.contains_key(&target) || index.targets_by_id.contains_key(&item.id) {
        return false;
    }
    index
        .active_targets
        .insert(target.clone(), (item.session_id.clone(), weight));
    index.targets_by_id.insert(item.id.clone(), target);
    index.counts.entry(item.session_id.clone()).or_default()[class(weight)] += 1;
    true
}

pub(super) fn release(item: &TransferItem) {
    let _ = release_id(&item.id);
}

/// The worker owns its reservation even after disconnect marks or trims its row.
pub(super) fn release_id(id: &str) -> Option<String> {
    let mut index = index().lock().unwrap();
    let target = index.targets_by_id.remove(id)?;
    let (session, weight) = index.active_targets.remove(&target)?;
    if let Some(counts) = index.counts.get_mut(&session) {
        counts[class(weight)] -= 1;
        if counts.iter().all(|&count| count == 0) {
            index.counts.remove(&session);
        }
    }
    Some(session)
}

fn class(weight: u32) -> usize {
    match weight {
        1 => 0,
        2 => 1,
        4 => 2,
        _ => 3,
    }
}

/// Number of distinct connections useful under the 32-permit memory budget.
pub(super) fn desired(uid: &str) -> usize {
    let index = index().lock().unwrap();
    let Some(counts) = index.counts.get(uid) else {
        return 0;
    };
    let mut permits = transfer_limit::PERMITS as usize;
    let mut sessions = 0;
    for (count, weight) in counts.iter().zip([1, 2, 4, 16]) {
        let take = (*count).min(permits / weight);
        sessions += take;
        permits -= take * weight;
    }
    let backlog: usize = counts.iter().sum();
    let setup_cap = match backlog {
        0..=16 => 4,
        17..=128 => 8,
        129..=1024 => 16,
        _ => 32,
    };
    sessions.min(setup_cap)
}

pub(super) fn push(item: &TransferItem, position: usize) {
    index()
        .lock()
        .unwrap()
        .by_id
        .insert(item.id.clone(), position);
}

pub(super) fn position(queue: &[TransferItem], id: &str) -> Option<usize> {
    let mut index = index().lock().unwrap();
    if let Some(&position) = index.by_id.get(id)
        && queue.get(position).is_some_and(|item| item.id == id)
    {
        return Some(position);
    }
    let position = queue.iter().position(|item| item.id == id)?;
    index.by_id.insert(id.to_string(), position);
    Some(position)
}

pub(super) fn rebuild(queue: &[TransferItem]) {
    let mut index = index().lock().unwrap();
    index.by_id.clear();
    for (position, item) in queue.iter().enumerate() {
        index.by_id.insert(item.id.clone(), position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, session: &str, path: &str) -> TransferItem {
        TransferItem {
            id: id.into(),
            session_id: session.into(),
            file_name: id.into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 0,
            total_bytes: 1,
            completed: false,
            error: None,
            local_path: id.into(),
            remote_path: path.into(),
        }
    }

    #[test]
    fn destination_reservation_is_scoped_to_session_and_released() {
        let first = item("one", "index-reservation-a", "/file");
        let duplicate = item("two", "index-reservation-a", "/file");
        let other = item("three", "index-reservation-b", "/file");
        assert!(reserve(&first));
        assert_eq!(desired("index-reservation-a"), 1);
        assert!(!reserve(&duplicate));
        assert!(reserve(&other));
        release(&first);
        assert_eq!(desired("index-reservation-a"), 0);
        assert!(reserve(&duplicate));
        release(&duplicate);
        release(&other);
    }

    #[test]
    fn downloads_share_a_local_destination_across_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = item("download-one", "index-download-a", "/file");
        first.direction = TransferDirection::Download;
        first.local_path = directory.path().join("file").to_string_lossy().into_owned();
        let mut second = first.clone();
        second.id = "download-two".into();
        second.session_id = "index-download-b".into();
        assert!(reserve(&first));
        assert!(!reserve(&second), "two writers would share the .part file");
        release(&first);
        second.local_path = directory
            .path()
            .join("./file")
            .to_string_lossy()
            .into_owned();
        #[cfg(windows)]
        {
            second.local_path = directory
                .path()
                .join("./FILE")
                .to_string_lossy()
                .into_owned();
        }
        assert!(reserve(&second));
        release(&first);
        assert!(
            !reserve(&first),
            "a stale completion must not release the new owner"
        );
        release(&second);
    }

    #[test]
    fn many_small_files_raise_connection_demand_within_budget() {
        let uid = "index-demand-small";
        let items: Vec<_> = (0..1100)
            .map(|n| {
                let mut row = item(&format!("small-{n}"), uid, &format!("/{n}"));
                row.total_bytes = 25 * 1024;
                row
            })
            .collect();
        for row in &items {
            assert!(reserve(row));
        }
        assert_eq!(desired(uid), 32);
        for row in &items {
            release(row);
        }
        assert_eq!(desired(uid), 0);
    }

    #[test]
    fn positions_rebuild_after_completed_rows_are_removed() {
        let mut queue = vec![
            item("index-row-a", "index-position", "/a"),
            item("index-row-b", "index-position", "/b"),
        ];
        rebuild(&queue);
        assert_eq!(position(&queue, "index-row-b"), Some(1));
        queue.remove(0);
        rebuild(&queue);
        assert_eq!(position(&queue, "index-row-b"), Some(0));
        assert_eq!(position(&queue, "index-row-a"), None);
    }
}
