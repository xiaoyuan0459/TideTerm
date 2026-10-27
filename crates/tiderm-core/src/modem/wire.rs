use super::Router;
use crate::protocol::Protocol;
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::mpsc;
pub(super) struct Wire {
    protocol: Arc<dyn Protocol>,
    rx: mpsc::Receiver<Vec<u8>>,
    buffered: VecDeque<u8>,
    pub router: Router,
    timeout: Duration,
}
impl Wire {
    pub fn new(
        protocol: Arc<dyn Protocol>,
        rx: mpsc::Receiver<Vec<u8>>,
        router: Router,
        timeout_secs: u32,
    ) -> Self {
        Self {
            protocol,
            rx,
            buffered: VecDeque::new(),
            router,
            timeout: Duration::from_secs(timeout_secs.into()),
        }
    }
    pub fn check(&self) -> Result<(), String> {
        if self.router.0.cancelled.load(Ordering::SeqCst) {
            Err("transfer cancelled".into())
        } else {
            Ok(())
        }
    }
    pub async fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.check()?;
        self.protocol.write(bytes).await.map_err(|e| e.to_string())
    }
    pub async fn abort(&self) -> Result<(), String> {
        // CAN burst is recognized by all three protocols. Do not encode as text.
        self.protocol
            .write(&[0x18; 8])
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn chunk(&mut self) -> Result<Vec<u8>, String> {
        self.check()?;
        if !self.buffered.is_empty() {
            return Ok(self.buffered.drain(..).collect());
        }
        let cancellation = self.router.0.cancel.notified();
        tokio::pin!(cancellation);
        cancellation.as_mut().enable();
        self.check()?;
        tokio::select! {
            _ = cancellation => Err("transfer cancelled".into()),
            result = tokio::time::timeout(self.timeout, self.rx.recv()) => match result {
                Ok(Some(bytes)) if !bytes.is_empty() && bytes.len() <= 1 << 20 => Ok(bytes),
                Ok(_) => Err("transport disconnected".into()),
                Err(_) => Err("Modem response timed out".into()),
            }
        }
    }
    pub async fn byte(&mut self) -> Result<u8, String> {
        self.check()?;
        if self.buffered.is_empty() {
            self.buffered = self.chunk().await?.into();
        }
        self.buffered
            .pop_front()
            .ok_or_else(|| "transport disconnected".into())
    }
    pub async fn exact(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::with_capacity(n);
        for _ in 0..n {
            bytes.push(self.byte().await?);
        }
        Ok(bytes)
    }
    pub fn unread(&mut self, bytes: &[u8]) {
        self.buffered.extend(bytes);
    }
    pub fn remainder(&mut self) -> Vec<u8> {
        self.rx.close();
        let mut out: Vec<u8> = self.buffered.drain(..).collect();
        while let Ok(bytes) = self.rx.try_recv() {
            out.extend(bytes);
        }
        out
    }
}
