//! Interrupted read cleanup contract.
use super::*;

struct HalfBrokenSftp {
    data: Vec<u8>,
}

/// 第一次 read 吐出前半段，之后固定报错 —— 模拟链路中断。
struct HalfBrokenRead {
    data: Vec<u8>,
    served: bool,
}

impl AsyncRead for HalfBrokenRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.served {
            return std::task::Poll::Ready(Err(std::io::Error::other("link reset")));
        }
        self.served = true;
        let half = self.data.len() / 2;
        buf.put_slice(&self.data[..half]);
        std::task::Poll::Ready(Ok(()))
    }
}

#[async_trait::async_trait]
impl SftpTransport for HalfBrokenSftp {
    async fn list_dir(&self, _path: &str) -> std::result::Result<Vec<SftpFileEntry>, String> {
        Ok(Vec::new())
    }
    async fn mkdir(&self, _path: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn remove_file(&self, _path: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn remove_dir(&self, _path: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn rename(&self, _from: &str, _to: &str) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn read_file(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        Err(format!("not used: {path}"))
    }
    async fn write_file(&self, _path: &str, _data: &[u8]) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn stat(&self, _path: &str) -> std::result::Result<SftpFileStat, String> {
        Ok(SftpFileStat {
            accessed_unix: None,
            size_bytes: self.data.len() as u64,
            modified_unix: 0,
            permissions: 0o644,
            is_dir: false,
            owner: None,
            group: None,
        })
    }
    async fn open_read_stream(
        &self,
        _path: &str,
    ) -> std::result::Result<Box<dyn AsyncRead + Unpin + Send>, String> {
        Ok(Box::new(HalfBrokenRead {
            data: self.data.clone(),
            served: false,
        }))
    }
    fn is_alive(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn download_failure_removes_partial_and_leaves_no_truncated_target() {
    let sftp = HalfBrokenSftp {
        data: b"0123456789abcdef".to_vec(),
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let local = dir.path().join("out.bin");
    let recorder = Recorder {
        events: Mutex::new(Vec::new()),
    };
    let err = download_stream(&sftp, "/remote/out.bin", &local, "s1", &recorder, None)
        .await
        .expect_err("half-broken stream must fail");
    assert!(err.contains("link reset"), "unexpected error: {err}");
    assert!(!local.exists(), "final path must stay absent after failure");
    assert!(
        !crate::sftp_stream::local_part_path(&local).exists(),
        ".part staging file must be cleaned up"
    );
}
