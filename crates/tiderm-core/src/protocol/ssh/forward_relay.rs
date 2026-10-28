//! Local endpoints and two-way stream relay, with deadlines and fixed buffers.
use super::{raw_channel::Channel, x11::X11};
use crate::error::{Error, Result};
use std::io::{Read, Write};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
type Local = Box<dyn Duplex>;
pub(super) fn agent_path() -> Result<String> {
    #[cfg(windows)]
    {
        Ok(std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| r"\\.\pipe\openssh-ssh-agent".into()))
    }
    #[cfg(unix)]
    {
        std::env::var("SSH_AUTH_SOCK")
            .map_err(|_| Error::Ssh("Agent forwarding requires SSH_AUTH_SOCK".into()))
    }
}
pub(super) async fn agent_connect(path: &str) -> Result<Local> {
    #[cfg(windows)]
    let stream = {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(path) {
                Ok(stream) => break Ok(Box::new(stream) as Local),
                Err(error)
                    if error.raw_os_error() == Some(231)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => break Err(error),
            }
        }
    };
    #[cfg(unix)]
    let stream = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::UnixStream::connect(path),
    )
    .await
    .map_err(|_| Error::Ssh("local SSH Agent connect timed out".into()))?
    .map(|s| Box::new(s) as Local);
    stream.map_err(|e| Error::Ssh(format!("connect local SSH Agent: {e}")))
}
async fn read_channel(mut channel: Channel) -> Result<Vec<u8>> {
    loop {
        let c = channel.clone();
        let (result, returned) = tokio::task::spawn_blocking(move || {
            let mut c = c;
            let mut buf = vec![0; 16384];
            let result = c.read(&mut buf).map(|n| {
                buf.truncate(n);
                buf
            });
            (result, c)
        })
        .await
        .map_err(|e| Error::Ssh(e.to_string()))?;
        channel = returned;
        match result {
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            other => return other.map_err(|e| Error::Ssh(e.to_string())),
        }
    }
}
async fn write_channel(channel: Channel, bytes: Vec<u8>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut offset = 0;
        while offset < bytes.len() {
            let mut c = channel.clone();
            let data = bytes[offset..].to_vec();
            let n = tokio::task::spawn_blocking(move || c.write(&data))
                .await
                .map_err(|e| Error::Ssh(e.to_string()))?;
            match n {
                Ok(0) => return Err(Error::Ssh("forward channel closed".into())),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    tokio::time::sleep(Duration::from_millis(2)).await
                }
                Err(e) => return Err(Error::Ssh(e.to_string())),
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::Ssh("forward write timed out".into()))?
}
pub(super) async fn relay(channel: Channel, x11: Option<X11>, agent: Option<String>) -> Result<()> {
    let mut prefix = Vec::new();
    let local: Local = if let Some(config) = x11 {
        // Authenticate before opening the local X server. Host/address in a remote
        // callback never selects the local destination.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let setup = loop {
            if let Some((used, setup)) = config.setup(&prefix)? {
                prefix.drain(..used);
                break setup;
            }
            let bytes = tokio::time::timeout_at(deadline, read_channel(channel.clone()))
                .await
                .map_err(|_| Error::Ssh("X11 setup timed out".into()))??;
            if bytes.is_empty() {
                return Err(Error::Ssh("X11 closed during setup".into()));
            }
            prefix.extend(bytes);
        };
        let connect = async {
            #[cfg(unix)]
            let local: Local = if config.local {
                Box::new(
                    tokio::net::UnixStream::connect(format!("/tmp/.X11-unix/X{}", config.display))
                        .await?,
                )
            } else {
                Box::new(
                    tokio::net::TcpStream::connect((config.host.as_str(), 6000 + config.display))
                        .await?,
                )
            };
            #[cfg(windows)]
            let local: Local = Box::new(
                tokio::net::TcpStream::connect((config.host.as_str(), 6000 + config.display))
                    .await?,
            );
            Ok::<Local, std::io::Error>(local)
        };
        let mut local = tokio::time::timeout(Duration::from_secs(3), connect)
            .await
            .map_err(|_| Error::Ssh("local X server connect timed out".into()))??;
        tokio::time::timeout(Duration::from_secs(10), local.write_all(&setup))
            .await
            .map_err(|_| Error::Ssh("local X11 setup timed out".into()))??;
        local
    } else {
        agent_connect(&agent.ok_or_else(|| Error::Ssh("unexpected forward channel".into()))?)
            .await?
    };
    let (mut reader, mut writer) = tokio::io::split(local);
    let outbound = async {
        let mut buf = [0; 16384];
        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                return Ok::<(), Error>(());
            }
            write_channel(channel.clone(), buf[..n].to_vec()).await?;
        }
    };
    let inbound = async {
        if !prefix.is_empty() {
            tokio::time::timeout(Duration::from_secs(30), writer.write_all(&prefix))
                .await
                .map_err(|_| Error::Ssh("local forward write timed out".into()))??;
        }
        loop {
            let bytes = read_channel(channel.clone()).await?;
            if bytes.is_empty() {
                return Ok::<(), Error>(());
            }
            tokio::time::timeout(Duration::from_secs(30), writer.write_all(&bytes))
                .await
                .map_err(|_| Error::Ssh("local forward write timed out".into()))??;
        }
    };
    tokio::select! { result = outbound => result, result = inbound => result }
}
