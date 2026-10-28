//! SFTP resources owned by a terminal session uid.

use super::{
    cancels, channels, ops, pool_capacity, pool_goals, transfer_index, transfer_pools, transfers,
    PoolSlots, SftpOp, SftpTransport,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tiderm_core::sftp_resume::PAUSED;

fn active() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Only one channel-open request may be in flight for a uid.
pub(super) fn begin_open(uid: &str) -> Option<Arc<AtomicBool>> {
    let mut active = active().lock().unwrap();
    if active.contains_key(uid) {
        return None;
    }
    let token = Arc::new(AtomicBool::new(true));
    active.insert(uid.to_string(), token.clone());
    Some(token)
}

pub(super) fn token(uid: &str) -> Option<Arc<AtomicBool>> {
    active().lock().unwrap().get(uid).cloned()
}

fn current(active: &HashMap<String, Arc<AtomicBool>>, uid: &str, token: &Arc<AtomicBool>) -> bool {
    token.load(Ordering::SeqCst)
        && active
            .get(uid)
            .is_some_and(|value| Arc::ptr_eq(value, token))
}

/// Serialize insertion with `close_session`: a late open cannot restore a closed channel.
pub(super) fn cache_channels(
    uid: &str,
    token: &Arc<AtomicBool>,
    pool: Vec<Arc<dyn SftpTransport>>,
) -> bool {
    let active = active().lock().unwrap();
    if !current(&active, uid, token) {
        return false;
    }
    let Some(primary) = pool.first().cloned() else {
        return false;
    };
    pool_goals()
        .lock()
        .unwrap()
        .insert(uid.to_string(), super::PoolGoal::default());
    transfer_pools()
        .lock()
        .unwrap()
        .insert(uid.to_string(), Vec::new());
    pool_capacity()
        .lock()
        .unwrap()
        .insert(uid.to_string(), Arc::new(PoolSlots::empty()));
    channels().lock().unwrap().insert(uid.to_string(), primary);
    true
}

pub(super) fn extend_channels(
    uid: &str,
    token: &Arc<AtomicBool>,
    generation: u64,
    workers: Vec<Arc<dyn SftpTransport>>,
) {
    let active = active().lock().unwrap();
    let mut goals = pool_goals().lock().unwrap();
    let Some(goal) = goals.get_mut(uid) else {
        return;
    };
    if !current(&active, uid, token) || goal.generation != generation {
        return;
    }
    goal.pending = goal.pending.saturating_sub(1);
    if transfer_index::desired(uid) == 0 {
        return;
    }
    let primary = channels().lock().unwrap().get(uid).cloned();
    let mut pools = transfer_pools().lock().unwrap();
    let Some(pool) = pools.get_mut(uid) else {
        return;
    };
    let mut added = workers;
    if !added.is_empty() {
        // An emergency primary-session fallback may already be active. Keep it
        // until the queue drains, when shrink_pool drops the entire pool.
        pool.extend(added.iter().cloned());
    } else if goal.pending == 0
        && pool.is_empty()
        && let Some(primary) = primary
    {
        tracing::warn!("SFTP transfer worker unavailable; using terminal channel");
        pool.push(primary.clone());
        added.push(primary);
    }
    if let Some(capacity) = pool_capacity().lock().unwrap().get(uid) {
        capacity.add(&added);
    }
    if goal.pending == 0 {
        goal.requested = pool.len();
    }
}

pub(super) fn open_failed(uid: &str, token: &Arc<AtomicBool>, error: SftpOp) {
    let mut active = active().lock().unwrap();
    if current(&active, uid, token) {
        ops().lock().unwrap().push(error);
        active.remove(uid);
        token.store(false, Ordering::SeqCst);
    }
}

/// Ignore directory results that finish after disconnect or reconnect.
pub(super) fn push_if_current(token: &Arc<AtomicBool>, op: SftpOp) {
    let active = active().lock().unwrap();
    if current(&active, &op.uid, token) {
        ops().lock().unwrap().push(op);
    }
}

/// Drop the cached channel, cancel transfers, and discard queued results for this uid.
pub fn close_session(uid: &str) {
    let mut active = active().lock().unwrap();
    if let Some(token) = active.remove(uid) {
        token.store(false, Ordering::SeqCst);
    }
    pool_goals().lock().unwrap().remove(uid);
    channels().lock().unwrap().remove(uid);
    transfer_pools().lock().unwrap().remove(uid);
    if let Some(capacity) = pool_capacity().lock().unwrap().remove(uid) {
        capacity.capacity.close();
    }
    super::transfer_limit::close_session(uid);
    ops().lock().unwrap().retain(|op| op.uid != uid);
    drop(active);

    let mut q = transfers().lock().unwrap();
    let cancel = cancels().lock().unwrap();
    let mut changed = false;
    for item in q
        .iter_mut()
        .filter(|item| item.session_id == uid && !item.completed)
    {
        if let Some(flag) = cancel.get(&item.id) {
            flag.set(PAUSED);
        }
        item.completed = true;
        item.error = Some("会话已断开".into());
        changed = true;
    }
    super::trim_completed(&mut q, 200);
    drop(cancel);
    drop(q);
    if changed {
        super::bump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::sftp::sftp_model::{TransferDirection, TransferItem};
    use crate::rust::sftp::transfer_control::TransferControl;
    use crate::rust::sftp::{cancel_transfer, OpKind};
    use std::sync::atomic::AtomicU64;
    use tiderm_core::sftp_resume::CANCELLED;

    fn uid() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!(
            "sftp-lifecycle-test-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn closing_session_cancels_transfer_and_discards_late_result() {
        let uid = uid();
        let open_token = begin_open(&uid).expect("open token");
        let transfer_id = format!("up-{uid}");
        let cancel = Arc::new(TransferControl::new());
        cancels()
            .lock()
            .unwrap()
            .insert(transfer_id.clone(), cancel.clone());
        transfers().lock().unwrap().push(TransferItem {
            id: transfer_id.clone(),
            session_id: uid.clone(),
            file_name: "test.txt".into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 0,
            total_bytes: 8,
            completed: false,
            error: None,
            local_path: "test.txt".into(),
            remote_path: "/test.txt".into(),
        });
        let op = SftpOp {
            kind: OpKind::List,
            uid: uid.clone(),
            path: "/".into(),
            ok: true,
            error: String::new(),
            entries: Vec::new(),
        };
        push_if_current(&open_token, op.clone());
        assert!(cancel_transfer(&transfer_id));
        assert_eq!(cancel.state.load(Ordering::SeqCst), CANCELLED);
        cancel.set(0);

        close_session(&uid);
        assert!(!open_token.load(Ordering::SeqCst));
        assert_eq!(cancel.state.load(Ordering::SeqCst), PAUSED);
        assert!(token(&uid).is_none());
        assert!(!ops().lock().unwrap().iter().any(|item| item.uid == uid));
        let q = transfers().lock().unwrap();
        let item = q.iter().find(|item| item.id == transfer_id).unwrap();
        assert!(item.completed);
        assert_eq!(item.error.as_deref(), Some("会话已断开"));
        drop(q);
        push_if_current(&open_token, op);
        assert!(!ops().lock().unwrap().iter().any(|item| item.uid == uid));
        transfers()
            .lock()
            .unwrap()
            .retain(|item| item.id != transfer_id);
        cancels().lock().unwrap().remove(&transfer_id);
    }
}
