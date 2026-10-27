use super::*;
use sftp_model::{TransferDirection, TransferItem};

#[test]
fn late_queue_entry_prevents_idle_pool_replacement() {
    let uid = "sftp-pool-shrink-race";
    let slots = Arc::new(PoolSlots::empty());
    pool_goals().lock().unwrap().insert(
        uid.into(),
        PoolGoal {
            requested: 1,
            ..PoolGoal::default()
        },
    );
    pool_capacity()
        .lock()
        .unwrap()
        .insert(uid.into(), slots.clone());
    let item = TransferItem {
        id: "race-transfer".into(),
        session_id: uid.into(),
        file_name: "race.bin".into(),
        direction: TransferDirection::Upload,
        bytes_transferred: 0,
        total_bytes: 1,
        completed: false,
        error: None,
        local_path: "race.bin".into(),
        remote_path: "/race.bin".into(),
    };
    assert!(transfer_index::reserve(&item));
    shrink_pool(uid);
    assert!(Arc::ptr_eq(
        pool_capacity().lock().unwrap().get(uid).unwrap(),
        &slots
    ));
    assert_eq!(pool_goals().lock().unwrap().get(uid).unwrap().requested, 1);
    transfer_index::release(&item);
    shrink_pool(uid);
    assert!(!Arc::ptr_eq(
        pool_capacity().lock().unwrap().get(uid).unwrap(),
        &slots
    ));
    pool_goals().lock().unwrap().remove(uid);
    pool_capacity().lock().unwrap().remove(uid);
}
