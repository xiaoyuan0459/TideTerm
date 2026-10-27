//! Bounded receipt times keyed by retained buffer line. The UI gets visible rows only.
use crate::bridge::context::context_ffi::LineTime;
use chrono::Timelike;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Annotations {
    stamps: BTreeMap<u64, u32>,
    visible: Vec<(u32, String)>,
}
impl Annotations {
    pub fn received(&mut self, before: u64, after: u64) {
        let stamp = chrono::Local::now().time().num_seconds_from_midnight();
        let start = before
            .min(after)
            .max(before.max(after).saturating_sub(2047));
        for line in start..=before.max(after) {
            self.stamps.insert(line, stamp);
        }
        while self.stamps.len() > 2048 {
            self.stamps.pop_first();
        }
    }
    pub fn observe(
        &mut self,
        spans: &mut [u32],
        gutter: u32,
        active_y: u32,
        cell_h: u32,
        blue: u32,
        magenta: u32,
    ) {
        let mut numbers: BTreeMap<u32, String> = BTreeMap::new();
        let occupied: BTreeSet<u32> = spans
            .as_chunks::<6>()
            .0
            .iter()
            .filter(|r| r[0] & 255 == 1 && (r[1] & 65535) >= gutter)
            .map(|r| r[1] >> 16)
            .collect();
        let first_baseline = spans
            .as_chunks::<6>()
            .0
            .iter()
            .find(|r| r[0] & 255 == 1 && (r[1] & 65535) < gutter)
            .map(|r| r[1] >> 16)
            .unwrap_or(0);
        for record in spans.as_chunks_mut::<6>().0 {
            if record[0] & 255 != 1 || (record[1] & 65535) >= gutter {
                continue;
            }
            let y = record[1] >> 16;
            let row = y.saturating_sub(first_baseline) / cell_h.max(1);
            if !occupied.contains(&y) && row != active_y {
                record[0] = 255; // Empty padding rows don't display gutter numbers.
                continue;
            }
            if let Some(c) = char::from_u32(record[4]).filter(|c| c.is_ascii_digit()) {
                numbers.entry(y).or_default().push(c);
                record[3] = if row == active_y { magenta } else { blue };
            }
        }
        self.visible = numbers
            .into_iter()
            .filter_map(|(y, number)| {
                number
                    .parse()
                    .ok()
                    .and_then(|n| self.stamps.get(&n))
                    .map(|s| {
                        (
                            y,
                            format!("[{:02}:{:02}:{:02}]", s / 3600, s / 60 % 60, s % 60),
                        )
                    })
            })
            .collect();
    }
    pub fn visible(&self, scale: f32) -> Vec<LineTime> {
        self.visible
            .iter()
            .map(|(y, stamp)| LineTime {
                baseline: *y as f32 / scale,
                stamp: stamp.clone(),
            })
            .collect()
    }
}

pub fn context_line_times(idx: u32) -> Vec<LineTime> {
    super::term::with_tab(idx, |st| st.annotations.visible(super::term::scale()))
}
