//! Bound active file transfers per SSH session so a large drop does not open
//! thousands of local and remote files at once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Semaphore;

/// Permit budget bounds memory while allowing many tiny files to overlap RTT.
pub(super) const PERMITS: u32 = 32;

/// Large files share bandwidth with few peers; tiny files need more concurrent
/// round trips. The buffer itself is capped to the remaining file size in core.
pub(super) fn weight(size_bytes: u64) -> u32 {
    match size_bytes {
        0 => 4, // size unknown until the remote metadata is fetched
        1..=65_536 => 1,
        65_537..=1_048_576 => 2,
        1_048_577..=33_554_432 => 4,
        _ => 16,
    }
}

fn limits() -> &'static Mutex<HashMap<String, Arc<Semaphore>>> {
    static LIMITS: OnceLock<Mutex<HashMap<String, Arc<Semaphore>>>> = OnceLock::new();
    LIMITS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn for_session(uid: &str) -> Arc<Semaphore> {
    limits()
        .lock()
        .unwrap()
        .entry(uid.to_string())
        .or_insert_with(|| Arc::new(Semaphore::new(PERMITS as usize)))
        .clone()
}

pub(super) fn close_session(uid: &str) {
    if let Some(limit) = limits().lock().unwrap().remove(uid) {
        limit.close();
    }
}

/// 压测探针（泄漏断言用）：指定前缀的会话信号量条目数。
#[cfg(test)]
pub(super) fn stress_residue(prefix: &str) -> usize {
    limits()
        .lock()
        .unwrap()
        .keys()
        .filter(|uid| uid.starts_with(prefix))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn permit_budget_waits_and_close_wakes_it() {
        let uid = "sftp-limit-test";
        let limit = for_session(uid);
        let mut permits = Vec::new();
        for _ in 0..PERMITS {
            permits.push(limit.clone().acquire_owned().await.unwrap());
        }
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(20),
            limit.clone().acquire_owned()
        )
        .await
        .is_err());
        close_session(uid);
        assert!(limit.acquire_owned().await.is_err());
        drop(permits);
    }

    #[test]
    fn file_size_controls_concurrency_cost() {
        assert_eq!(weight(25 * 1024), 1);
        assert_eq!(weight(1024 * 1024), 2);
        assert_eq!(weight(5_000_000_000), 16);
        assert_eq!(weight(0), 4);
    }

    #[tokio::test]
    async fn two_large_files_fill_the_memory_budget() {
        let uid = "sftp-large-weight-test";
        let limit = for_session(uid);
        let first = limit.clone().acquire_many_owned(16).await.unwrap();
        let second = limit.clone().acquire_many_owned(16).await.unwrap();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(20),
            limit.clone().acquire_owned()
        )
        .await
        .is_err());
        drop(first);
        assert!(limit.clone().acquire_owned().await.is_ok());
        drop(second);
        close_session(uid);
    }
}
