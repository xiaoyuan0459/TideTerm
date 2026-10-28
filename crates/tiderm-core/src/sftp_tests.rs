//! sftp.rs 的测试模块（行数冻结门禁拆出；`#[path]` 回链）。

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// In-memory mock transport backed by a `HashMap<path, bytes>`.
struct MockSftp {
    alive: AtomicBool,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockSftp {
    fn new() -> Self {
        Self {
            alive: AtomicBool::new(true),
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// AsyncWrite adapter that buffers into a `Vec` and commits to the mock
/// map on flush (so `upload_stream` round-trips through the mock).
struct MockWriteStream {
    path: String,
    buf: Vec<u8>,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl AsyncWrite for MockWriteStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let me = self.get_mut();
        me.buf.extend_from_slice(data);
        me.files
            .lock()
            .unwrap()
            .insert(me.path.clone(), me.buf.clone());
        std::task::Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let me = self.get_mut();
        me.files
            .lock()
            .unwrap()
            .insert(me.path.clone(), me.buf.clone());
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

#[async_trait::async_trait]
impl SftpTransport for MockSftp {
    async fn list_dir(&self, _path: &str) -> std::result::Result<Vec<SftpFileEntry>, String> {
        Ok(Vec::new())
    }
    async fn mkdir(&self, _path: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn remove_file(&self, path: &str) -> std::result::Result<(), String> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
    async fn remove_dir(&self, _path: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn rename(&self, from: &str, to: &str) -> std::result::Result<(), String> {
        let mut files = self.files.lock().unwrap();
        match files.remove(from) {
            Some(data) => {
                files.insert(to.to_string(), data);
                Ok(())
            }
            None => Err(format!("file not found: {from}")),
        }
    }
    async fn read_file(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| format!("file not found: {path}"))
    }
    async fn write_file(&self, path: &str, data: &[u8]) -> std::result::Result<(), String> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), data.to_vec());
        Ok(())
    }
    async fn stat(&self, path: &str) -> std::result::Result<SftpFileStat, String> {
        let files = self.files.lock().unwrap();
        let size = files
            .get(path)
            .map(|d| d.len() as u64)
            .ok_or_else(|| format!("file not found: {path}"))?;
        Ok(SftpFileStat {
            accessed_unix: None,
            size_bytes: size,
            modified_unix: 0,
            permissions: 0o644,
            is_dir: false,
            owner: None,
            group: None,
        })
    }
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    async fn open_read_stream(
        &self,
        path: &str,
    ) -> std::result::Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        let data = self.read_file(path).await?;
        Ok(Box::new(std::io::Cursor::new(data)))
    }
    async fn open_write_stream(
        &self,
        path: &str,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        Ok(Box::new(MockWriteStream {
            path: path.to_string(),
            buf: Vec::new(),
            files: self.files.clone(),
        }))
    }
    async fn open_write_stream_at(
        &self,
        path: &str,
        offset: u64,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        let mut data = self
            .files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default();
        data.truncate(offset as usize);
        Ok(Box::new(MockWriteStream {
            path: path.to_string(),
            buf: data,
            files: self.files.clone(),
        }))
    }
}

struct PauseAfterChunk<'a>(&'a std::sync::atomic::AtomicU8);

#[async_trait::async_trait]
impl ProgressSink for PauseAfterChunk<'_> {
    async fn progress(&self, event: SftpTransferProgress) -> Result<(), String> {
        if event.bytes_transferred > 0 && event.bytes_transferred < event.total_bytes {
            self.0.store(crate::sftp_resume::PAUSED, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[tokio::test]
async fn resumable_upload_preserves_part_and_completes_after_pause() {
    use crate::sftp_resume::{upload_resumable, RUNNING};
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("source.bin");
    let bytes = vec![7u8; 9 * 1024 * 1024];
    tokio::fs::write(&local, &bytes).await.unwrap();
    let sftp = MockSftp::new();
    let control = std::sync::atomic::AtomicU8::new(RUNNING);
    let err = upload_resumable(
        &sftp,
        &local,
        "/source.bin",
        "s",
        &PauseAfterChunk(&control),
        &control,
    )
    .await
    .unwrap_err();
    assert_eq!(err, "transfer paused");
    let partial = sftp.stat("/source.bin.part").await.unwrap().size_bytes;
    assert!(partial > 0 && partial < bytes.len() as u64);
    control.store(RUNNING, Ordering::SeqCst);
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    upload_resumable(&sftp, &local, "/source.bin", "s", &recorder, &control)
        .await
        .unwrap();
    assert_eq!(sftp.read_file("/source.bin").await.unwrap(), bytes);
    assert!(sftp.stat("/source.bin.part").await.is_err());
    assert_eq!(
        recorder.events.lock().unwrap()[0].bytes_transferred,
        partial
    );
}

#[tokio::test]
async fn resumable_download_uses_existing_verified_prefix() {
    use crate::sftp_resume::{download_resumable, RUNNING};
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("target.bin");
    let bytes = vec![3u8; 2 * 1024 * 1024];
    let sftp = MockSftp::new();
    sftp.write_file("/target.bin", &bytes).await.unwrap();
    tokio::fs::write(dir.path().join("target.bin.part"), &bytes[..1024 * 1024])
        .await
        .unwrap();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let control = std::sync::atomic::AtomicU8::new(RUNNING);
    download_resumable(&sftp, "/target.bin", &local, "s", &recorder, &control)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(local).await.unwrap(), bytes);
    assert_eq!(
        recorder.events.lock().unwrap()[0].bytes_transferred,
        1024 * 1024
    );
}

#[tokio::test]
async fn resumable_upload_rejects_mismatched_staging_without_replacing_target() {
    use crate::sftp_resume::{upload_resumable, RUNNING};
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("source.bin");
    tokio::fs::write(&local, vec![1u8; 128 * 1024])
        .await
        .unwrap();
    let sftp = MockSftp::new();
    sftp.write_file("/source.bin.part", &vec![2u8; 64 * 1024])
        .await
        .unwrap();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let control = std::sync::atomic::AtomicU8::new(RUNNING);
    let err = upload_resumable(&sftp, &local, "/source.bin", "s", &recorder, &control)
        .await
        .unwrap_err();
    assert!(err.contains("differs"));
    assert!(sftp.stat("/source.bin").await.is_err());
    assert_eq!(
        sftp.stat("/source.bin.part").await.unwrap().size_bytes,
        64 * 1024
    );
}

#[tokio::test]
async fn resumable_upload_checks_staging_head_even_when_tail_matches() {
    use crate::sftp_resume::{upload_resumable, RUNNING};
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("source.bin");
    tokio::fs::write(&local, vec![1u8; 256 * 1024])
        .await
        .unwrap();
    let mut staged = vec![1u8; 128 * 1024];
    staged[..64 * 1024].fill(2);
    let sftp = MockSftp::new();
    sftp.write_file("/source.bin.part", &staged).await.unwrap();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let control = std::sync::atomic::AtomicU8::new(RUNNING);
    let err = upload_resumable(&sftp, &local, "/source.bin", "s", &recorder, &control)
        .await
        .unwrap_err();
    assert!(err.contains("differs"));
    assert!(sftp.stat("/source.bin").await.is_err());
}

#[tokio::test]
async fn resumable_download_cancel_removes_staging() {
    use crate::sftp_resume::{download_resumable, CANCELLED};
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("target.bin");
    let part = dir.path().join("target.bin.part");
    tokio::fs::write(&part, b"partial").await.unwrap();
    let sftp = MockSftp::new();
    sftp.write_file("/target.bin", b"full content")
        .await
        .unwrap();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let control = std::sync::atomic::AtomicU8::new(CANCELLED);
    let err = download_resumable(&sftp, "/target.bin", &local, "s", &recorder, &control)
        .await
        .unwrap_err();
    assert_eq!(err, "transfer cancelled");
    assert!(!part.exists());
    assert!(!local.exists());
}

/// Collects progress events for assertions.
struct Recorder {
    events: Mutex<Vec<SftpTransferProgress>>,
}

#[async_trait::async_trait]
impl ProgressSink for Recorder {
    async fn progress(&self, p: SftpTransferProgress) -> std::result::Result<(), String> {
        self.events.lock().unwrap().push(p);
        Ok(())
    }
}

#[tokio::test]
async fn upload_stream_writes_remote_and_reports_progress() {
    // Force single-chunk (small file) so the final progress event fires.
    let dir = tempfile::tempdir().expect("tempdir");
    let local = dir.path().join("hello.txt");
    tokio::fs::write(&local, b"hello world")
        .await
        .expect("write local");

    let sftp = MockSftp::new();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    upload_stream(&sftp, &local, "/remote/hello.txt", "s1", &recorder, None)
        .await
        .expect("upload");

    assert_eq!(
        sftp.read_file("/remote/hello.txt").await.unwrap(),
        b"hello world"
    );
    // R-07：暂存文件在成功路径上必须已被 rename 收走。
    assert!(!sftp
        .files
        .lock()
        .unwrap()
        .contains_key("/remote/hello.txt.part"));
    let events = recorder.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].direction, TransferDirection::Upload);
    assert_eq!(events[0].bytes_transferred, 11);
    assert_eq!(events[0].total_bytes, 11);
}

#[tokio::test]
async fn upload_stream_respects_cancel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let local = dir.path().join("big.bin");
    tokio::fs::write(&local, vec![0u8; 1024 * 1024])
        .await
        .expect("write local");

    let sftp = MockSftp::new();
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let cancel = AtomicBool::new(true);
    let result = upload_stream(
        &sftp,
        &local,
        "/remote/big.bin",
        "s1",
        &recorder,
        Some(&cancel),
    )
    .await;
    assert_eq!(result, Err("transfer cancelled".to_string()));
    // R-07：取消后远程不允许留下截断的暂存文件。
    assert!(!sftp
        .files
        .lock()
        .unwrap()
        .contains_key("/remote/big.bin.part"));
}

#[tokio::test]
async fn download_stream_writes_local_and_reports_progress() {
    let sftp = MockSftp::new();
    sftp.write_file("/remote/hello.txt", b"hello world")
        .await
        .expect("seed");

    let dir = tempfile::tempdir().expect("tempdir");
    let local = dir.path().join("out.txt");
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    download_stream(&sftp, "/remote/hello.txt", &local, "s1", &recorder, None)
        .await
        .expect("download");

    assert_eq!(tokio::fs::read(&local).await.unwrap(), b"hello world");
    // R-07：成功路径不留 .part 暂存文件。
    assert!(!crate::sftp_stream::local_part_path(&local).exists());
    let events = recorder.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].direction, TransferDirection::Download);
}

#[path = "sftp_tests/bench.rs"]
mod bench;
/// 读到一半报错的远程流：下载必须清理 .part 且不在最终路径留截断文件。
#[path = "sftp_tests/stream_failures.rs"]
mod stream_failures;
