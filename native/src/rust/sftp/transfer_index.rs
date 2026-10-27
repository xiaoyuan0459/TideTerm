//! O(1) lookup for large transfer queues. Call only while holding the queue lock.

use super::sftp_model::{TransferDirection, TransferItem};
use super::transfer_limit;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type Target = (String, bool, String);

#[derive(Default)]
struct Index {
    by_id: HashMap<String, usize>,
    active_targets: HashMap<Target, u32>,
    counts: HashMap<String, [usize; 4]>,
}

fn index() -> &'static Mutex<Index> {
    static INDEX: OnceLock<Mutex<Index>> = OnceLock::new();
    INDEX.get_or_init(|| Mutex::new(Index::default()))
}

fn target(item: &TransferItem) -> Target {
    match item.direction {
        TransferDirection::Upload => (item.session_id.clone(), true, item.remote_path.clone()),
        TransferDirection::Download => (item.session_id.clone(), false, item.local_path.clone()),
    }
}

pub(super) fn reserve(item: &TransferItem) -> bool {
    let mut index = index().lock().unwrap();
    let weight = transfer_limit::weight(item.total_bytes);
    if index.active_targets.contains_key(&target(item)) {
        return false;
    }
    index.active_targets.insert(target(item), weight);
    index.counts.entry(item.session_id.clone()).or_default()[class(weight)] += 1;
    true
}

pub(super) fn release(item: &TransferItem) {
    let mut index = index().lock().unwrap();
    if let Some(weight) = index.active_targets.remove(&target(item))
        && let Some(counts) = index.counts.get_mut(&item.session_id)
    {
        counts[class(weight)] -= 1;
    }
}

pub(super) fn forget_session(uid: &str) {
    let mut index = index().lock().unwrap();
    index
        .active_targets
        .retain(|(session, _, _), _| session != uid);
    index.counts.remove(uid);
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
        forget_session("index-reservation-a");
        release(&other);
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
