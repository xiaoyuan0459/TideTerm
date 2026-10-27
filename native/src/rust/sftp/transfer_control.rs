//! Transfer state with a wakeup for queued tasks.

use std::sync::atomic::{AtomicU8, Ordering};
use tokio::sync::watch;

pub(super) struct TransferControl {
    pub state: AtomicU8,
    sender: watch::Sender<u8>,
}

impl TransferControl {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(tiderm_core::sftp_resume::RUNNING);
        Self {
            state: AtomicU8::new(tiderm_core::sftp_resume::RUNNING),
            sender,
        }
    }

    pub fn set(&self, state: u8) {
        self.state.store(state, Ordering::SeqCst);
        self.sender.send_replace(state);
    }

    pub async fn wait_stopped(&self) {
        let mut receiver = self.sender.subscribe();
        while *receiver.borrow_and_update() == tiderm_core::sftp_resume::RUNNING {
            if receiver.changed().await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queued_transfer_wakes_on_pause() {
        let control = std::sync::Arc::new(TransferControl::new());
        let waiting = control.clone();
        let task = tokio::spawn(async move { waiting.wait_stopped().await });
        tokio::task::yield_now().await;
        control.set(tiderm_core::sftp_resume::PAUSED);
        tokio::time::timeout(std::time::Duration::from_millis(100), task)
            .await
            .expect("queue waiter should wake")
            .unwrap();
    }
}
