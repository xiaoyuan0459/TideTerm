//! Completed read-only provider snapshots outlive ordinary draft edits.
//! Pending requests and exact-name receipts retain their strict input binding.
use super::Snapshot;
use crate::bridge::completion::completion_ffi::ShellReport;

#[derive(Default)]
pub(super) struct CatalogPool {
    snapshots: Vec<Snapshot>,
    states: [u8; 6],
}
fn kind(report: &ShellReport) -> Option<usize> {
    if report.name_catalog {
        Some(0)
    } else if report.git {
        Some(1)
    } else if !report.docker_context.is_empty() {
        Some(2)
    } else if !report.kube.config.is_empty() {
        Some(3 + usize::from(report.kube.resource))
    } else {
        None
    }
}
impl CatalogPool {
    pub fn context(&self) -> (u8, u8, u8) {
        let Some(last) = self.snapshots.last() else {
            return (0, 0, 0);
        };
        let mut result = context(&last.report);
        for snapshot in self
            .snapshots
            .iter()
            .rev()
            .filter(|s| s.report.platform == result.0)
        {
            let other = context(&snapshot.report);
            if result.1 == 0 {
                result.1 = other.1;
            }
            if result.2 == 0 {
                result.2 = other.2;
            }
        }
        result
    }
    pub fn remove_provider(&mut self, provider: usize) {
        if provider >= self.states.len() {
            return;
        }
        self.snapshots.retain(|s| kind(&s.report) != Some(provider));
        self.states[provider] = 0;
    }
    pub fn insert(&mut self, snapshot: &Snapshot) {
        let Some(kind) = kind(&snapshot.report) else {
            return;
        };
        let mut snapshot = snapshot.clone();
        snapshot.report.command.clear();
        self.snapshots
            .retain(|s| self::kind(&s.report) != Some(kind));
        self.snapshots.push(snapshot);
        self.states[kind] = 3;
    }
    pub fn validate(&mut self, generation: u32, directory: u64) {
        self.snapshots.retain(|snapshot| {
            let Some(kind) = kind(&snapshot.report) else {
                return false;
            };
            let state = if snapshot.generation != generation
                || snapshot.directory_revision != Some(directory)
            {
                6
            } else if snapshot.at.elapsed() >= std::time::Duration::from_secs(60) {
                7
            } else {
                3
            };
            self.states[kind] = state;
            state == 3
        });
    }
    fn view(snapshot: &Snapshot, source: u32) -> ShellReport {
        let mut report = snapshot.report.clone();
        report.source = source;
        report.state = 3;
        report.age_seconds = snapshot.at.elapsed().as_secs() as u32;
        report
    }
    pub fn poll(&self, source: u32, provider: u8) -> ShellReport {
        let Some(index) = provider.checked_sub(1).map(usize::from).filter(|i| *i < 6) else {
            return ShellReport::default();
        };
        self.snapshots
            .iter()
            .find(|s| kind(&s.report) == Some(index))
            .map(|s| Self::view(s, source))
            .unwrap_or_else(|| ShellReport {
                source,
                state: self.states[index],
                ..Default::default()
            })
    }
    pub fn receipt(&self, source: u32, receipt: &str) -> Option<ShellReport> {
        self.snapshots
            .iter()
            .find(|s| s.report.token == receipt)
            .map(|s| Self::view(s, source))
    }
    pub fn retire(&mut self, receipt: &str, state: u8) {
        self.snapshots.retain(|snapshot| {
            if snapshot.report.token != receipt {
                return true;
            }
            if let Some(kind) = kind(&snapshot.report) {
                self.states[kind] = state;
            }
            false
        });
    }
}
pub(super) fn context(report: &ShellReport) -> (u8, u8, u8) {
    (
        report.platform,
        if report.name_catalog {
            if report.platform == 1 {
                1
            } else {
                2
            }
        } else {
            0
        },
        if report.system == 0 && report.platform == 2 {
            1
        } else {
            report.system
        },
    )
}
