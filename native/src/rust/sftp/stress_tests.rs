//! SFTP 生命周期长时压测（句柄/注册表泄漏探针）。
//!
//! 背景（docs/reports/sftp-repair-2026-09-25.md）：close_session 负责「关缓存
//! channel + 取消排队任务 + 丢弃旧会话迟到结果」，但从未做过长时运行下的资源
//! 泄漏验证。本文件用进程内 fake transport 循环压：
//! - 快速档（默认随套件跑）：数百轮完整开/关循环，逐轮断言该 uid 在全部全局
//!   注册表（active/channels/pools/goals/capacity/limits/ops/queue/cancels）
//!   零残留——任何一条泄漏立即失败；
//! - 浸泡档（`#[ignore]`，发布前手动跑）：数万轮 + 交错「开通道途中关闭」的
//!   迟到路径，覆盖 cache_channels 与 close_session 的竞态面。
//!
//! 断言按 `sftp-stress-` 前缀扫描而不是全局长度差值：cargo test 并行下其他
//! 测试会合理地增减共享注册表，前缀扫描才是并发安全的泄漏判据。

use super::sftp_model::{TransferDirection, TransferItem};
use super::transfer_control::TransferControl;
use super::{
    cancels, channels, close_session, lifecycle, ops, pool_capacity, pool_goals, transfer_index,
    transfer_pools, transfers, OpKind, SftpOp,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tiderm_core::sftp::SftpTransport;
use tiderm_core::sftp_resume::PAUSED;

struct FakeTransport;

#[async_trait::async_trait]
impl SftpTransport for FakeTransport {
    async fn list_dir(&self, _: &str) -> Result<Vec<tiderm_core::sftp::SftpFileEntry>, String> {
        Err("fake".into())
    }
    async fn mkdir(&self, _: &str) -> Result<(), String> {
        Err("fake".into())
    }
    async fn remove_file(&self, _: &str) -> Result<(), String> {
        Err("fake".into())
    }
    async fn remove_dir(&self, _: &str) -> Result<(), String> {
        Err("fake".into())
    }
    async fn rename(&self, _: &str, _: &str) -> Result<(), String> {
        Err("fake".into())
    }
    async fn read_file(&self, _: &str) -> Result<Vec<u8>, String> {
        Err("fake".into())
    }
    async fn write_file(&self, _: &str, _: &[u8]) -> Result<(), String> {
        Err("fake".into())
    }
    async fn stat(&self, _: &str) -> Result<tiderm_core::sftp::SftpFileStat, String> {
        Err("fake".into())
    }
    fn is_alive(&self) -> bool {
        true
    }
}

fn queue_transfer(uid: &str, id: &str) -> Arc<TransferControl> {
    let control = Arc::new(TransferControl::new());
    cancels()
        .lock()
        .unwrap()
        .insert(id.to_string(), control.clone());
    transfers().lock().unwrap().push(TransferItem {
        id: id.to_string(),
        session_id: uid.to_string(),
        file_name: "stress.bin".into(),
        direction: TransferDirection::Upload,
        bytes_transferred: 0,
        total_bytes: 8,
        completed: false,
        error: None,
        local_path: "stress.bin".into(),
        remote_path: "/stress.bin".into(),
    });
    control
}

fn list_op(uid: &str) -> SftpOp {
    SftpOp {
        kind: OpKind::List,
        uid: uid.to_string(),
        path: "/".into(),
        ok: true,
        error: String::new(),
        entries: Vec::new(),
    }
}

/// 该 uid 在（除传输队列外的）全部全局注册表中必须零残留。
/// 传输队列单列：排队条目在 close 后仍按设计保留至测试清理/trim。
fn assert_no_residue(uid: &str) {
    assert!(
        !channels().lock().unwrap().contains_key(uid),
        "channels 残留 {uid}"
    );
    assert!(
        !transfer_pools().lock().unwrap().contains_key(uid),
        "transfer_pools 残留 {uid}"
    );
    assert!(
        !pool_goals().lock().unwrap().contains_key(uid),
        "pool_goals 残留 {uid}"
    );
    assert!(
        !pool_capacity().lock().unwrap().contains_key(uid),
        "pool_capacity 残留 {uid}"
    );
    assert!(
        !ops().lock().unwrap().iter().any(|op| op.uid == uid),
        "ops 队列残留 {uid}"
    );
    assert!(
        !cancels().lock().unwrap().keys().any(|id| id.contains(uid)),
        "cancels 残留 {uid}"
    );
    assert_eq!(transfer_index::desired(uid), 0, "transfer_index 残留 {uid}");
}

/// 完整生命周期一轮：开通道 → 排队传输 → 送达目录结果 → 关闭 → 迟到结果丢弃。
fn full_cycle(n: usize) {
    let uid = format!("sftp-stress-full-{n}");
    let token = lifecycle::begin_open(&uid).expect("begin_open");
    let primary: Arc<dyn SftpTransport> = Arc::new(FakeTransport);
    assert!(lifecycle::cache_channels(&uid, &token, vec![primary]));

    let id = format!("up-{uid}");
    let control = queue_transfer(&uid, &id);
    let op = list_op(&uid);
    lifecycle::push_if_current(&token, op.clone());

    close_session(&uid);

    assert!(!token.load(Ordering::SeqCst));
    assert!(lifecycle::token(&uid).is_none());
    assert_eq!(
        control.state.load(Ordering::SeqCst),
        PAUSED,
        "排队任务应被置为 PAUSED（保留断点语义）"
    );
    {
        let q = transfers().lock().unwrap();
        let item = q.iter().find(|item| item.id == id).expect("队列条目");
        assert!(item.completed);
        assert_eq!(item.error.as_deref(), Some("会话已断开"));
    }
    // 迟到的目录结果必须被丢弃。
    lifecycle::push_if_current(&token, op);
    assert!(!ops().lock().unwrap().iter().any(|item| item.uid == uid));

    // 清理测试自留的队列/取消条目（真实路径由 finish_transfer 回收），
    // 并按 trim 路径重建位置索引，保持共享结构对其他测试干净。
    transfers().lock().unwrap().retain(|item| item.id != id);
    cancels().lock().unwrap().remove(&id);
    let q = transfers().lock().unwrap();
    transfer_index::rebuild(&q);
    drop(q);

    assert_no_residue(&uid);
    assert!(
        !transfers()
            .lock()
            .unwrap()
            .iter()
            .any(|item| item.session_id == uid),
        "transfers 队列残留 {uid}"
    );
}

/// 开通道途中关闭：close 先到，迟到的 cache_channels 必须被拒（不得复活已关会话）。
fn late_open_cycle(n: usize) {
    let uid = format!("sftp-stress-late-{n}");
    let token = lifecycle::begin_open(&uid).expect("begin_open");
    close_session(&uid);
    let primary: Arc<dyn SftpTransport> = Arc::new(FakeTransport);
    assert!(
        !lifecycle::cache_channels(&uid, &token, vec![primary]),
        "关闭后的迟到开通道不得复活会话"
    );
    assert_no_residue(&uid);
}

/// 关闭幂等：重复 close 不 panic、不产生新状态。
fn double_close_cycle(n: usize) {
    let uid = format!("sftp-stress-double-{n}");
    let token = lifecycle::begin_open(&uid).expect("begin_open");
    let primary: Arc<dyn SftpTransport> = Arc::new(FakeTransport);
    assert!(lifecycle::cache_channels(&uid, &token, vec![primary]));
    close_session(&uid);
    close_session(&uid);
    assert_no_residue(&uid);
}

/// `sftp-stress-` 前缀在全部注册表的全量扫描（并发安全版终检）。
fn assert_no_stress_residue() {
    let residue = |desc: &str, found: bool| {
        assert!(!found, "压测会话在 {desc} 有泄漏残留");
    };
    residue(
        "channels",
        channels()
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.starts_with("sftp-stress-")),
    );
    residue(
        "transfer_pools",
        transfer_pools()
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.starts_with("sftp-stress-")),
    );
    residue(
        "pool_goals",
        pool_goals()
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.starts_with("sftp-stress-")),
    );
    residue(
        "pool_capacity",
        pool_capacity()
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.starts_with("sftp-stress-")),
    );
    residue(
        "transfer_limit",
        super::transfer_limit::stress_residue("sftp-stress-") > 0,
    );
    residue(
        "ops",
        ops()
            .lock()
            .unwrap()
            .iter()
            .any(|op| op.uid.starts_with("sftp-stress-")),
    );
    residue(
        "transfers",
        transfers()
            .lock()
            .unwrap()
            .iter()
            .any(|item| item.session_id.starts_with("sftp-stress-")),
    );
    residue(
        "cancels",
        cancels()
            .lock()
            .unwrap()
            .keys()
            .any(|id| id.contains("sftp-stress-")),
    );
}

#[test]
fn stress_repeated_lifecycles_leave_no_residue() {
    for n in 0..300 {
        full_cycle(n);
        match n % 3 {
            1 => late_open_cycle(n),
            2 => double_close_cycle(n),
            _ => {}
        }
    }
    assert_no_stress_residue();
}

/// 长时浸泡档（默认忽略；发布前 `cargo test -- --ignored` 手动跑）：
/// 数万轮交错路径 + 阶段性全量扫描。进程内纯内存循环，覆盖注册表增长类泄漏；
/// 真实 socket 句柄回收由 transport Drop + 30s 无进展上限兜底。
#[test]
#[ignore = "长时浸泡压测：发布前手动跑 cargo test -- --ignored"]
fn soak_repeated_lifecycles_stable_over_tens_of_thousands() {
    for n in 0..20_000 {
        full_cycle(n);
        match n % 4 {
            0 => late_open_cycle(n),
            1 => double_close_cycle(n),
            _ => {}
        }
        if n % 5_000 == 4_999 {
            assert_no_stress_residue();
        }
    }
    assert_no_stress_residue();
}
