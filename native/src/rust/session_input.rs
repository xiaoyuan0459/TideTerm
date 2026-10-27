//! One bounded FIFO writer per transport. Keyboard input must never race itself.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tiderm_core::protocol::Protocol;
use tokio::sync::Notify;

const MAX_BYTES: usize = 1 << 20;
const MAX_MESSAGES: usize = 4096;
const BATCH_BYTES: usize = 64 << 10;

#[derive(Default)]
struct Pending {
    chunks: VecDeque<Input>,
    bytes: usize,
    closed: bool,
}

enum Input {
    Write(Vec<u8>),
    Resize(u16, u16),
    Modem(tiderm_core::modem::Job),
}

#[derive(Default)]
pub(super) struct SessionInput {
    pending: Arc<Mutex<Pending>>,
    ready: Arc<Notify>,
    worker: RefCell<Option<tokio::task::JoinHandle<()>>>,
}

impl SessionInput {
    pub fn transfer(&self, job: tiderm_core::modem::Job) -> Result<(), &'static str> {
        let mut pending = self.pending.lock().unwrap();
        if pending.closed || pending.chunks.len() >= MAX_MESSAGES {
            return Err("会话输入已关闭或队列已满");
        }
        pending.chunks.push_back(Input::Modem(job));
        drop(pending);
        self.ready.notify_one();
        Ok(())
    }
    pub fn write(&self, data: &[u8]) -> Result<(), &'static str> {
        if data.is_empty() {
            return Ok(());
        }
        let mut pending = self.pending.lock().unwrap();
        if pending.closed {
            return Err("会话输入已关闭");
        }
        if pending.bytes.saturating_add(data.len()) > MAX_BYTES
            || pending.chunks.len() >= MAX_MESSAGES
        {
            return Err("输入缓冲已满（上限 1 MiB / 4096 项），请等待后重试");
        }
        pending.bytes += data.len();
        pending.chunks.push_back(Input::Write(data.to_vec()));
        drop(pending);
        self.ready.notify_one();
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), &'static str> {
        let mut pending = self.pending.lock().unwrap();
        if pending.closed {
            return Err("会话输入已关闭");
        }
        if let Some(Input::Resize(old_cols, old_rows)) = pending.chunks.back_mut() {
            *old_cols = cols;
            *old_rows = rows;
        } else {
            if pending.chunks.len() >= MAX_MESSAGES {
                return Err("终端尺寸更新队列已满");
            }
            pending.chunks.push_back(Input::Resize(cols, rows));
        }
        drop(pending);
        self.ready.notify_one();
        Ok(())
    }

    pub fn attach(&self, uid: String, protocol: Arc<dyn Protocol>) {
        let pending = self.pending.clone();
        let ready = self.ready.clone();
        assert!(
            self.worker.borrow().is_none(),
            "input writer already attached"
        );
        *self.worker.borrow_mut() = Some(super::session::shared_runtime().spawn(async move {
            loop {
                // Register before examining the queue; Notify retains one permit
                // when input arrives between releasing the lock and awaiting.
                let notification = ready.notified();
                let operation = {
                    let mut state = pending.lock().unwrap();
                    if state.closed {
                        return;
                    }
                    match state.chunks.pop_front() {
                        Some(Input::Write(mut batch)) => {
                            while let Some(Input::Write(front)) = state.chunks.front() {
                                if batch.len() + front.len() > BATCH_BYTES {
                                    break;
                                }
                                if let Some(Input::Write(chunk)) = state.chunks.pop_front() {
                                    batch.extend(chunk);
                                }
                            }
                            Some(Input::Write(batch))
                        }
                        other => other,
                    }
                };
                let Some(operation) = operation else {
                    notification.await;
                    continue;
                };
                let (result, bytes, writing) = match operation {
                    Input::Write(data) => (protocol.write(&data).await, data.len(), true),
                    Input::Resize(cols, rows) => (protocol.resize(cols, rows).await, 0, false),
                    Input::Modem(job) => {
                        job.run(protocol.clone()).await;
                        drop(job);
                        (Ok(()), 0, false)
                    }
                };
                {
                    let mut state = pending.lock().unwrap();
                    state.bytes = state.bytes.saturating_sub(bytes);
                    if writing && result.is_err() {
                        state.closed = true;
                        state.chunks.clear();
                        state.bytes = 0;
                    }
                }
                if let Err(error) = result {
                    super::session::emit_error(&uid, &format!("会话输入失败：{error}"));
                    if writing {
                        return;
                    }
                }
            }
        }));
    }

    pub fn close(&self) {
        let mut pending = self.pending.lock().unwrap();
        pending.closed = true;
        pending.chunks.clear();
        pending.bytes = 0;
        drop(pending);
        if let Some(worker) = self.worker.borrow_mut().take() {
            worker.abort();
        }
    }
}

impl Drop for SessionInput {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Transport(Mutex<Vec<u8>>);
    #[async_trait::async_trait]
    impl Protocol for Transport {
        async fn read(&self) -> tiderm_core::error::Result<Vec<u8>> {
            Ok(Vec::new())
        }
        async fn write(&self, data: &[u8]) -> tiderm_core::error::Result<()> {
            tokio::task::yield_now().await;
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(())
        }
        async fn resize(&self, cols: u16, rows: u16) -> tiderm_core::error::Result<()> {
            self.0
                .lock()
                .unwrap()
                .extend_from_slice(format!("#{cols}x{rows}#").as_bytes());
            Ok(())
        }
        fn is_alive(&self) -> bool {
            true
        }
        fn exit_code(&self) -> Option<u32> {
            None
        }
    }

    #[test]
    fn buffered_and_connected_input_preserve_transport_order_with_bounded_memory() {
        let input = SessionInput::default();
        let transport = Arc::new(Transport(Mutex::new(Vec::new())));
        input.write(b"before-connect:").unwrap();
        input.resize(12, 34).unwrap();
        assert!(input.write(&vec![0; MAX_BYTES]).is_err());
        input.attach("input-order-test".into(), transport.clone());
        let expected = b"before-connect:#12x34#Write-Output 'HISTORY_DIRECT'\r";
        for byte in b"Write-Output 'HISTORY_DIRECT'\r" {
            input.write(&[*byte]).unwrap();
        }
        super::super::session::shared_runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    if transport.0.lock().unwrap().len() == expected.len() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
        });
        assert_eq!(*transport.0.lock().unwrap(), expected);
        input.close();
        assert!(input.write(b"after-close").is_err());
    }
}
