//! Bounded system sampling; no terminal input or UI dependencies.
use crate::error::{Error, Result};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct SystemSample {
    pub cpu_percent: Option<f32>,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[async_trait::async_trait]
pub trait SystemMonitor: Send + Sync {
    async fn sample(&self) -> Result<SystemSample>;
}

struct LocalMonitor;

pub fn local_monitor() -> Arc<dyn SystemMonitor> {
    Arc::new(LocalMonitor)
}

#[async_trait::async_trait]
impl SystemMonitor for LocalMonitor {
    async fn sample(&self) -> Result<SystemSample> {
        tokio::task::spawn_blocking(|| {
            static SYSTEM: OnceLock<Mutex<(sysinfo::System, Option<std::time::Instant>, bool)>> =
                OnceLock::new();
            let mut state = SYSTEM
                .get_or_init(|| Mutex::new((sysinfo::System::new(), None, false)))
                .lock()
                .map_err(|_| Error::Other("system sampler lock poisoned".into()))?;
            // Multiple visible panes share one CPU interval and one system allocation.
            let now = std::time::Instant::now();
            let fresh = state.1.is_none();
            if fresh
                || state
                    .1
                    .is_some_and(|at| now.duration_since(at).as_secs_f32() >= 1.0)
            {
                state.0.refresh_cpu();
                state.0.refresh_memory();
                state.1 = Some(now);
                state.2 = !fresh;
            }
            Ok(SystemSample {
                cpu_percent: state.2.then(|| state.0.global_cpu_info().cpu_usage()),
                total_bytes: state.0.total_memory(),
                used_bytes: state.0.used_memory(),
            })
        })
        .await
        .map_err(|e| Error::Other(format!("system sample task: {e}")))?
    }
}

/// Linux /proc contract: ignore guest fields (already included in user/nice).
pub(crate) fn linux_sample(text: &str, previous: &mut Option<(u64, u64)>) -> Result<SystemSample> {
    let cpu = text
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| Error::Other("remote system does not expose Linux /proc/stat".into()))?;
    let values: Vec<u64> = cpu
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| Error::Other("invalid remote CPU counters".into()))?;
    if values.len() < 4 {
        return Err(Error::Other("incomplete remote CPU counters".into()));
    }
    let total = values
        .iter()
        .try_fold(0u64, |sum, value| sum.checked_add(*value))
        .ok_or_else(|| Error::Other("remote CPU counter overflow".into()))?;
    let idle = values[3].saturating_add(values.get(4).copied().unwrap_or(0));
    let memory = |key: &str| -> Result<u64> {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|value| value.checked_mul(1024))
            .ok_or_else(|| Error::Other(format!("missing remote {key}")))
    };
    let total_bytes = memory("MemTotal:")?;
    let available = memory("MemAvailable:")?;
    if total_bytes == 0 || available > total_bytes {
        return Err(Error::Other("invalid remote memory counters".into()));
    }
    let cpu_percent = previous.and_then(|(old_total, old_idle)| {
        let delta = total.checked_sub(old_total)?;
        let idle_delta = idle.checked_sub(old_idle)?;
        (delta > 0 && idle_delta <= delta)
            .then(|| 100.0 * (delta - idle_delta) as f32 / delta as f32)
    });
    *previous = Some((total, idle));
    Ok(SystemSample {
        cpu_percent,
        used_bytes: total_bytes - available,
        total_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_samples_use_deltas_and_reject_invalid_or_reset_counters() {
        let mut previous = None;
        let first = linux_sample(
            "cpu 100 0 50 850 0 0 0 0 999 999\nMemTotal: 1024 kB\nMemAvailable: 256 kB",
            &mut previous,
        )
        .unwrap();
        assert_eq!(first.cpu_percent, None);
        assert_eq!(first.used_bytes, 768 * 1024);
        let second = linux_sample(
            "cpu 125 0 75 900 0 0 0 0\nMemTotal: 1024 kB\nMemAvailable: 512 kB",
            &mut previous,
        )
        .unwrap();
        assert_eq!(second.cpu_percent, Some(50.0));
        assert!(linux_sample(
            "cpu 1 0 1 8\nMemTotal: 1024 kB\nMemAvailable: 512 kB",
            &mut previous
        )
        .unwrap()
        .cpu_percent
        .is_none());
        assert!(linux_sample(
            "cpu 1 0 1 8\nMemTotal: 1024 kB\nMemAvailable: 2048 kB",
            &mut previous
        )
        .is_err());
        assert!(linux_sample("not Linux", &mut previous).is_err());
    }
}
