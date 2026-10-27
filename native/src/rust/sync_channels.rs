//! Window-scoped explicit keyboard recipients; never used by Sender draft acceptance.
use crate::bridge::sync::sync_ffi::{SyncChannel, SyncStatus};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

const MAX_VIEWS: usize = 512;
#[derive(Default)]
struct Channel {
    name: String,
    members: BTreeSet<u32>,
}
struct Owner {
    views: BTreeSet<u32>,
    channels: [Channel; 4],
}
thread_local! {
    static OWNERS: RefCell<BTreeMap<u64, Owner>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

pub fn term_sync_create() -> u64 {
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if owners.len() >= 64 {
            return 0;
        }
        let id = NEXT.with(|next| {
            let id = next.get();
            if let Some(value) = id.checked_add(1) {
                next.set(value);
                id
            } else {
                0
            }
        });
        if id != 0 {
            owners.insert(
                id,
                Owner {
                    views: BTreeSet::new(),
                    channels: std::array::from_fn(|i| Channel {
                        name: (i + 1).to_string(),
                        ..Default::default()
                    }),
                },
            );
        }
        id
    })
}

pub fn term_sync_close(owner: u64) {
    OWNERS.with(|owners| owners.borrow_mut().remove(&owner));
}

pub fn term_sync_update(owner: u64, views: &[u32]) -> bool {
    if views.len() > MAX_VIEWS
        || !super::term::TABS.with(|tabs| {
            let tabs = tabs.borrow();
            views.iter().all(|id| tabs.contains_key(id))
        })
    {
        return false;
    }
    let views: BTreeSet<_> = views.iter().copied().collect();
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if owners
            .iter()
            .any(|(&id, other)| id != owner && !other.views.is_disjoint(&views))
        {
            return false;
        }
        let Some(state) = owners.get_mut(&owner) else {
            return false;
        };
        if state.views != views {
            for channel in &mut state.channels {
                channel.members.retain(|id| views.contains(id));
            }
            state.views = views;
        }
        true
    })
}

pub fn term_sync_snapshot(owner: u64) -> Vec<SyncChannel> {
    OWNERS.with(|owners| {
        owners
            .borrow()
            .get(&owner)
            .map(|state| {
                state
                    .channels
                    .iter()
                    .map(|channel| SyncChannel {
                        name: channel.name.clone(),
                        members: channel.members.iter().copied().collect(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Replacing all four channels is atomic; bad names or stale/duplicate members keep the old state.
pub fn term_sync_replace(owner: u64, channels: &[SyncChannel]) -> u8 {
    if super::term_ops::input_locked() {
        return 6;
    }
    if channels.len() != 4 {
        return 3;
    }
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let Some(state) = owners.get_mut(&owner) else {
            return 7;
        };
        let mut names = BTreeSet::new();
        let mut members = BTreeSet::new();
        let mut next: [Channel; 4] = std::array::from_fn(|_| Channel::default());
        for (input, channel) in channels.iter().zip(&mut next) {
            let name = input.name.trim();
            if name.is_empty()
                || name.len() > 256
                || name.chars().count() > 64
                || name.chars().any(char::is_control)
                || !names.insert(name.to_owned())
            {
                return 3;
            }
            if input.members.len() > MAX_VIEWS
                || input
                    .members
                    .iter()
                    .any(|id| !state.views.contains(id) || !members.insert(*id))
            {
                return 4;
            }
            channel.name = name.to_owned();
            channel.members = input.members.iter().copied().collect();
        }
        state.channels = next;
        0
    })
}

pub fn term_sync_status(owner: u64, source: u32) -> SyncStatus {
    OWNERS.with(|owners| {
        owners
            .borrow()
            .get(&owner)
            .and_then(|state| {
                state
                    .channels
                    .iter()
                    .enumerate()
                    .find(|(_, channel)| channel.members.contains(&source))
                    .map(|(index, channel)| SyncStatus {
                        channel: index as u8 + 1,
                        name: channel.name.clone(),
                        members: channel.members.len() as u32,
                    })
            })
            .unwrap_or_default()
    })
}

pub(super) fn recipients(source: u32) -> Vec<u32> {
    OWNERS.with(|owners| {
        for owner in owners.borrow().values() {
            for channel in &owner.channels {
                if channel.members.contains(&source) {
                    return channel
                        .members
                        .iter()
                        .copied()
                        .filter(|id| *id != source)
                        .collect();
                }
            }
        }
        Vec::new()
    })
}

pub(super) fn retire(source: u32) {
    OWNERS.with(|owners| {
        for owner in owners.borrow_mut().values_mut() {
            owner.views.remove(&source);
            for channel in &mut owner.channels {
                channel.members.remove(&source);
            }
        }
    });
}
