//! Local sink memory/throughput benchmark; does not connect to an SSH server.
use super::*;

// ── Bench: large-file and many-small-file upload speed + peak memory ──
struct SinkSftp;

#[async_trait::async_trait]
impl SftpTransport for SinkSftp {
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
    async fn read_file(&self, _path: &str) -> std::result::Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
    async fn write_file(&self, _path: &str, _data: &[u8]) -> std::result::Result<(), String> {
        Ok(())
    }
    async fn stat(&self, _path: &str) -> std::result::Result<SftpFileStat, String> {
        Ok(SftpFileStat {
            accessed_unix: None,
            size_bytes: 0,
            modified_unix: 0,
            permissions: 0o644,
            is_dir: false,
            owner: None,
            group: None,
        })
    }
    fn is_alive(&self) -> bool {
        true
    }
    async fn open_write_stream(
        &self,
        _path: &str,
    ) -> std::result::Result<Box<dyn AsyncWrite + Unpin + Send>, String> {
        Ok(Box::new(tokio::io::sink()))
    }
}

struct MemRecorder {
    peak: Mutex<u64>,
}

fn current_memory_kb() -> u64 {
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(std::process::id());
    sys.refresh_process(pid);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

#[async_trait::async_trait]
impl ProgressSink for MemRecorder {
    async fn progress(&self, _p: SftpTransferProgress) -> std::result::Result<(), String> {
        let mem = current_memory_kb();
        let mut peak = self.peak.lock().unwrap();
        if mem > *peak {
            *peak = mem;
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "memory/speed benchmark: run with --ignored --nocapture"]
async fn bench_upload_memory_and_speed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sftp = SinkSftp;

    // Large file: 256 MiB.
    let large = dir.path().join("large.bin");
    tokio::fs::write(&large, vec![0u8; 256 * 1024 * 1024])
        .await
        .expect("write large local");
    let mem = MemRecorder {
        peak: Mutex::new(0),
    };
    let start = std::time::Instant::now();
    upload_stream(&sftp, &large, "/remote/large.bin", "s1", &mem, None)
        .await
        .expect("upload large");
    let large_elapsed = start.elapsed();
    let large_mbps = (256.0 * 1024.0 * 1024.0) / large_elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "[bench] large upload: 256 MiB in {:?} -> {large_mbps:.2} MB/s, peak mem {:.1} MiB",
        large_elapsed,
        *mem.peak.lock().unwrap() as f64 / 1024.0 / 1024.0
    );

    // 5000 small files: 25 KiB each (125 MiB total).
    let small_dir = dir.path().join("small");
    std::fs::create_dir_all(&small_dir).expect("mkdir small");
    let small_payload = vec![0xABu8; 25 * 1024];
    for i in 0..5000 {
        tokio::fs::write(small_dir.join(format!("f{i:05}.bin")), &small_payload)
            .await
            .expect("write small local");
    }
    let mem2 = MemRecorder {
        peak: Mutex::new(0),
    };
    let start2 = std::time::Instant::now();
    for i in 0..5000 {
        let local = small_dir.join(format!("f{i:05}.bin"));
        let remote = format!("/remote/small/f{i:05}.bin");
        upload_stream(&sftp, &local, &remote, "s1", &mem2, None)
            .await
            .expect("upload small");
    }
    let small_elapsed = start2.elapsed();
    let small_mbps = (5000.0 * 25.0 * 1024.0) / small_elapsed.as_secs_f64() / 1_000_000.0;
    let files_per_sec = 5000.0 / small_elapsed.as_secs_f64();
    eprintln!(
        "[bench] 5000 small upload: 5000 files / 125 MiB in {:?} -> {small_mbps:.2} MB/s, {files_per_sec:.1} files/s, peak mem {:.1} MiB",
        small_elapsed,
        *mem2.peak.lock().unwrap() as f64 / 1024.0 / 1024.0
    );
}
