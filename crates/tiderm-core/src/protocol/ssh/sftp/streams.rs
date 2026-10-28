use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
const READ_CHUNK: usize = 1024 * 1024;

/// File close also performs libssh2 I/O, so it shares the same mode gate.
pub(super) struct SftpStreamFile {
    file: Mutex<Option<ssh2::File>>,
    session: ssh2::Session,
    gate: Arc<Mutex<()>>,
}

impl SftpStreamFile {
    pub(super) fn new(file: ssh2::File, session: ssh2::Session, gate: Arc<Mutex<()>>) -> Self {
        Self {
            file: Mutex::new(Some(file)),
            session,
            gate,
        }
    }

    pub(super) fn with_file<T>(
        &self,
        op: impl FnOnce(&mut ssh2::File) -> io::Result<T>,
    ) -> io::Result<T> {
        let _mode = SftpCallGuard::new(&self.session, &self.gate).map_err(io::Error::other)?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("SFTP file lock poisoned"))?;
        let file = file
            .as_mut()
            .ok_or_else(|| io::Error::other("SFTP file already closed"))?;
        op(file)
    }
}

impl Drop for SftpStreamFile {
    fn drop(&mut self) {
        let file = match self.file.get_mut() {
            Ok(file) => file.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(file) = file {
            let mode = SftpCallGuard::new(&self.session, &self.gate).ok();
            drop(file);
            drop(mode);
        }
    }
}

/// AsyncWrite adapter over a blocking libssh2 SFTP file.
pub(super) struct LibSftpWriteStream {
    pub(super) inner: SftpStreamFile,
}

impl AsyncWrite for LibSftpWriteStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        Poll::Ready(tokio::task::block_in_place(|| {
            this.inner.with_file(|file| retry_io(|| file.write(buf)))
        }))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Poll::Ready(tokio::task::block_in_place(|| {
            this.inner.with_file(|file| retry_io(|| file.flush()))
        }))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// AsyncRead adapter over a blocking libssh2 SFTP file.
pub(super) struct LibSftpReadStream {
    pub(super) inner: SftpStreamFile,
}

impl AsyncRead for LibSftpReadStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let this = self.get_mut();
        let dst = buf.initialize_unfilled();
        let limit = dst.len().min(READ_CHUNK);
        let n = tokio::task::block_in_place(|| {
            this.inner
                .with_file(|file| retry_io(|| file.read(&mut dst[..limit])))
        });
        match n {
            Ok(0) => Poll::Ready(Ok(())), // EOF
            Ok(n) => {
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
