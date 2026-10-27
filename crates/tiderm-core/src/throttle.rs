//! Master-password brute-force throttle (S1).
//!
//! 复刻自旧 Tauri 后端 `src-tauri/src/throttle.rs`：连续 [`MAX_FAILURES`]
//! 次验证失败后进入锁定窗口，锁定时长按指数退避（30s → 60s → …，封顶
//! 5 分钟），验证成功后完全复位。
//!
//! 状态**只存内存**——落盘会让本地攻击者得以篡改文件对保险库实施 DoS。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Consecutive failures allowed before the first lockout.
pub const MAX_FAILURES: u32 = 5;
/// Initial lockout duration in seconds.
const BASE_LOCKOUT_SECS: u64 = 30;
/// Maximum lockout duration in seconds (5 minutes).
const MAX_LOCKOUT_SECS: u64 = 300;

/// Runtime-only throttle state.
struct ThrottleGuard {
    /// Consecutive failed attempts since the last success/reset.
    failures: AtomicU32,
    /// Number of lockouts entered; drives exponential backoff.
    escalations: AtomicU32,
    /// Instant until which verification is refused.
    locked_until: Mutex<Instant>,
}

static THROTTLE: LazyLock<ThrottleGuard> = LazyLock::new(|| ThrottleGuard {
    failures: AtomicU32::new(0),
    locked_until: Mutex::new(Instant::now()),
    escalations: AtomicU32::new(0),
});

/// Returns the remaining lockout in seconds, or `None` when not throttled.
pub fn throttle_remaining() -> Option<u64> {
    let until = *THROTTLE.locked_until.lock().unwrap();
    let now = Instant::now();
    (now < until).then(|| (until - now).as_secs() + 1)
}

/// Standard lockout error message (shared by all throttled entry points).
pub fn lockout_error(remaining: u64) -> String {
    format!("too many failed attempts, try again in {remaining}s")
}

/// Record a failed verification; engages lockout after [`MAX_FAILURES`].
pub fn record_failure() {
    let failures = THROTTLE.failures.fetch_add(1, Ordering::SeqCst) + 1;
    if failures >= MAX_FAILURES {
        let escalations = THROTTLE.escalations.fetch_add(1, Ordering::SeqCst);
        let secs = BASE_LOCKOUT_SECS
            .saturating_mul(2u64.saturating_pow(escalations))
            .min(MAX_LOCKOUT_SECS);
        *THROTTLE.locked_until.lock().unwrap() = Instant::now() + Duration::from_secs(secs);
        THROTTLE.failures.store(0, Ordering::SeqCst);
        // 审计事件：旧版 record_failure 在锁定时调用 `app_log::audit_log`。
        crate::app_log::audit_log("master_password_throttle_engaged", &[]);
        tracing::warn!(
            target: "security",
            lockout_secs = secs,
            "master password throttle engaged"
        );
    }
}

/// Record a successful verification; resets the throttle completely.
pub fn record_success() {
    THROTTLE.failures.store(0, Ordering::SeqCst);
    THROTTLE.escalations.store(0, Ordering::SeqCst);
}

/// Test-only: reset the global throttle state (single-threaded test isolation).
#[cfg(test)]
pub fn reset() {
    THROTTLE.failures.store(0, Ordering::SeqCst);
    THROTTLE.escalations.store(0, Ordering::SeqCst);
    *THROTTLE.locked_until.lock().unwrap() = Instant::now();
}

#[cfg(test)]
pub(crate) fn escalations() -> u32 {
    THROTTLE.escalations.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn set_escalations(v: u32) {
    THROTTLE.escalations.store(v, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn force_unlock() {
    *THROTTLE.locked_until.lock().unwrap() = Instant::now();
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 单条用例覆盖完整生命周期——全局静态状态并行测试易竞态。
    #[test]
    fn throttle_full_lifecycle() {
        reset();

        // Below threshold: no lockout.
        for _ in 0..(MAX_FAILURES - 1) {
            record_failure();
        }
        assert!(throttle_remaining().is_none());

        // 5th failure engages the lockout (base 30 s).
        record_failure();
        let remaining = throttle_remaining().expect("should be locked out");
        assert!(remaining <= BASE_LOCKOUT_SECS + 1, "remaining: {remaining}");
        // Failure counter resets after lockout engages.
        assert_eq!(escalations(), 1);

        // Second lockout escalates (60 s).
        force_unlock();
        for _ in 0..MAX_FAILURES {
            record_failure();
        }
        let remaining2 = throttle_remaining().expect("should be locked out again");
        assert!(
            remaining2 > BASE_LOCKOUT_SECS,
            "escalated lockout should exceed base: {remaining2}"
        );
        assert!(remaining2 <= BASE_LOCKOUT_SECS * 2 + 1);

        // Success resets everything.
        force_unlock();
        record_success();
        assert!(throttle_remaining().is_none());
        assert_eq!(escalations(), 0);

        // Escalation caps at MAX_LOCKOUT_SECS.
        reset();
        set_escalations(32); // way past cap
        for _ in 0..MAX_FAILURES {
            record_failure();
        }
        let capped = throttle_remaining().expect("should be locked out");
        assert!(capped <= MAX_LOCKOUT_SECS + 1, "capped: {capped}");

        reset();
    }

    #[test]
    fn lockout_error_message_is_stable() {
        assert_eq!(
            lockout_error(42),
            "too many failed attempts, try again in 42s"
        );
    }
}
