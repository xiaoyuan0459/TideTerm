//! Route wire bytes before output batching or text conversion; retain one reader.
use super::session::SessionHost;
use std::sync::Arc;
use tiderm_core::{
    modem::{Request, Router},
    protocol::Protocol,
    read_loop::ReadSource,
};
pub(super) struct ProtoSource(pub Arc<dyn Protocol>, pub Router);
#[async_trait::async_trait]
impl ReadSource for ProtoSource {
    async fn read(&self) -> Result<Vec<u8>, String> {
        loop {
            let bytes = match self.0.read().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.1.eof();
                    return Err(error.to_string());
                }
            };
            if bytes.is_empty() {
                self.1.eof();
                return Ok(bytes);
            }
            if let Some(bytes) = self.1.route(bytes).await {
                return Ok(bytes);
            }
        }
    }
    fn is_alive(&self) -> bool {
        self.0.is_alive()
    }
}
impl SessionHost {
    pub fn start_modem(&self, request: Request, timeout: u32) -> Result<(), String> {
        if !self.alive() {
            return Err("session is not connected".into());
        }
        self.input
            .transfer(self.modem.reserve(request, timeout)?)
            .map_err(str::to_owned)
    }
}
