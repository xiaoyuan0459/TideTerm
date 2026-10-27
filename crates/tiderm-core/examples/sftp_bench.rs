//! Temporary live LAN probe; credentials are read only from process environment.
#[path = "sftp_bench/host_verification.rs"]
mod host_verification;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiderm_core::protocol::ssh::LibSshSftp;
use tiderm_core::protocol::ssh::{SshAuth, SshConfig, SshInstance};
use tiderm_core::protocol::Protocol;
use tiderm_core::sftp::{ProgressSink, SftpTransferProgress, SftpTransport};
use tiderm_core::sftp_resume::{download_resumable, upload_resumable, PAUSED, RUNNING};

struct Meter {
    control: Arc<AtomicU8>,
    pause_at: u64,
    start: Instant,
    last: Mutex<Instant>,
}

struct Quiet;

struct RejectUnknown;

#[async_trait::async_trait]
impl tiderm_core::auth::HostKeyInquirer for RejectUnknown {
    async fn confirm(
        &self,
        _session_id: &str,
        _host: &str,
        _port: u16,
        _key_type: &str,
        _fingerprint: &str,
        _key: &[u8],
    ) -> Result<bool, String> {
        Ok(false)
    }
}

#[async_trait::async_trait]
impl ProgressSink for Quiet {
    async fn progress(&self, _progress: SftpTransferProgress) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProgressSink for Meter {
    async fn progress(&self, progress: SftpTransferProgress) -> Result<(), String> {
        if self.pause_at > 0
            && progress.bytes_transferred >= self.pause_at
            && progress.bytes_transferred < progress.total_bytes
        {
            self.control.store(PAUSED, Ordering::SeqCst);
        }
        let mut last = self.last.lock().unwrap();
        if last.elapsed().as_secs() >= 10 || progress.bytes_transferred == progress.total_bytes {
            println!(
                "PROGRESS bytes={} total={} seconds={:.1}",
                progress.bytes_transferred,
                progress.total_bytes,
                self.start.elapsed().as_secs_f64()
            );
            *last = Instant::now();
        }
        Ok(())
    }
}

fn meter(control: Arc<AtomicU8>, pause_at: u64) -> Meter {
    Meter {
        control,
        pause_at,
        start: Instant::now(),
        last: Mutex::new(Instant::now()),
    }
}

fn extra_channels(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    count: usize,
) -> Result<Vec<Arc<dyn SftpTransport>>, Box<dyn std::error::Error>> {
    let mut channels: Vec<Arc<dyn SftpTransport>> = Vec::new();
    for _ in 1..count {
        let socket = std::net::TcpStream::connect((host, port))?;
        socket.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
        socket.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
        let mut worker = ssh2::Session::new()?;
        worker.set_tcp_stream(socket);
        worker.handshake()?;
        host_verification::require_known(&worker, host, port)?;
        worker.userauth_password(user, password)?;
        let raw = worker.sftp()?;
        if std::env::var_os("TIDETERM_BENCH_SESSION_TIMEOUT").is_some() {
            worker.set_timeout(30_000);
        }
        worker.set_blocking(false);
        channels.push(Arc::new(LibSshSftp::new(
            worker,
            raw,
            Arc::new(AtomicBool::new(true)),
            Arc::new(Mutex::new(())),
        )));
    }
    Ok(channels)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::var("TIDETERM_SSH_HOST")?;
    let user = std::env::var("TIDETERM_SSH_USER")?;
    let password = std::env::var("TIDETERM_SSH_PASSWORD")?;
    let mode = std::env::args().nth(1).unwrap_or_else(|| "smoke".into());
    let port: u16 = std::env::var("TIDETERM_SSH_PORT")
        .unwrap_or_else(|_| "22".into())
        .parse()?;
    let socket = std::net::TcpStream::connect((host.as_str(), port))?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    socket.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut session = ssh2::Session::new()?;
    session.set_tcp_stream(socket);
    session.handshake()?;
    host_verification::require_known(&session, &host, port)?;
    session.userauth_password(&user, &password)?;
    if mode == "cleanup" {
        let target_pid = std::env::args().nth(2).ok_or("cleanup requires a PID")?;
        if target_pid.is_empty() || !target_pid.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("cleanup PID must contain digits only".into());
        }
        let mut cleanup = session.channel_session()?;
        cleanup.exec(&format!(
            "rm -rf -- '/tmp/tideterm-sftp-bench-{target_pid}'"
        ))?;
        let mut discard = Vec::new();
        cleanup.read_to_end(&mut discard)?;
        cleanup.wait_close()?;
        println!("CLEANED /tmp/tideterm-sftp-bench-{target_pid}");
        return Ok(());
    }
    {
        let mut info = session.channel_session()?;
        info.exec("df -B1 /tmp | tail -1; for x in /sys/class/net/*/speed; do printf '%s ' \"$x\"; cat \"$x\" 2>/dev/null; done")?;
        let mut output = String::new();
        info.read_to_string(&mut output)?;
        info.wait_close()?;
        println!("REMOTE_INFO {}", output.replace('\n', " | "));
    }
    if mode == "raw" {
        let mut channel = session.channel_session()?;
        channel.exec("head -c 268435456 /dev/zero")?;
        let mut buffer = vec![0; 1024 * 1024];
        let mut count = 0u64;
        let started = Instant::now();
        loop {
            let n = channel.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
        }
        channel.wait_close()?;
        println!(
            "RAW_DOWNLOAD bytes={count} seconds={:.1} mibps={:.1}",
            started.elapsed().as_secs_f64(),
            count as f64 / started.elapsed().as_secs_f64() / 1048576.0
        );
        return Ok(());
    }
    let raw = session.sftp()?;
    session.set_blocking(false);
    let sftp: Arc<dyn SftpTransport> = Arc::new(LibSshSftp::new(
        session.clone(),
        raw,
        Arc::new(AtomicBool::new(true)),
        Arc::new(Mutex::new(())),
    ));
    let root = format!("/tmp/tideterm-sftp-bench-{}", std::process::id());
    sftp.mkdir(&root).await?;
    let local_dir = PathBuf::from(".sftp-bench-tmp");
    std::fs::create_dir_all(&local_dir)?;
    println!("READY pid={} root={root} mode={mode}", std::process::id());
    let bench_sessions: usize = std::env::var("TIDETERM_BENCH_SESSIONS")
        .unwrap_or_else(|_| "4".into())
        .parse()?;
    if !(1..=32).contains(&bench_sessions) {
        return Err("benchmark session count must be 1..=32".into());
    }
    let mut production_instance = None;
    let mut production_channels = None;
    if mode == "pool-live" || mode == "small-production" {
        let config = SshConfig {
            host: host.clone(),
            port,
            username: user.clone(),
            auth: SshAuth::Password {
                password: password.clone(),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 0,
            reconnect_interval_secs: 0,
            link_watchdog_secs: 0,
            options: Default::default(),
            term: "xterm-256color".into(),
        };
        let instance = SshInstance::connect(
            config,
            80,
            24,
            "sftp-pool-bench".into(),
            &RejectUnknown,
            Arc::new(tiderm_core::auth::NoopKbBackend),
        )
        .await?;
        let mut pool = vec![instance.open_sftp().await?];
        pool.extend(instance.open_sftp_workers(bench_sessions - 1).await?);
        println!("PRODUCTION_POOL sessions={}", pool.len());
        if pool.len() != bench_sessions {
            return Err("production SFTP worker count mismatch".into());
        }
        if mode == "pool-live" {
            for (index, channel) in pool.iter().enumerate() {
                let path = format!("{root}/pool-{index}.bin");
                channel.write_file(&path, &[index as u8; 4096]).await?;
                if channel.stat(&path).await?.size_bytes != 4096 {
                    return Err("pool file size mismatch".into());
                }
            }
            instance.disconnect().await?;
        } else {
            production_channels = Some(pool);
            production_instance = Some(instance);
        }
    }
    let mut channels = production_channels.unwrap_or_else(|| vec![sftp.clone()]);
    if mode == "small-production" && std::env::var_os("TIDETERM_BENCH_SKIP_PRIMARY").is_some() {
        channels.remove(0);
    }
    if mode == "parallel" || mode == "small-parallel" || mode == "small-download" {
        channels.extend(extra_channels(
            &host,
            port,
            &user,
            &password,
            bench_sessions,
        )?);
    }
    if mode == "parallel" {
        let size = 128 * 1024 * 1024u64;
        let local = local_dir.join("parallel-source.bin");
        std::fs::File::create(&local)?.set_len(size)?;
        println!("PARALLEL_READY sessions={}", channels.len());
        let started = Instant::now();
        let mut tasks = tokio::task::JoinSet::new();
        for (index, channel) in channels.iter().cloned().enumerate() {
            let source = local.clone();
            let remote = format!("{root}/parallel-{index}.bin");
            tasks.spawn(async move {
                upload_resumable(
                    channel.as_ref(),
                    &source,
                    &remote,
                    "bench",
                    &Quiet,
                    &AtomicU8::new(RUNNING),
                )
                .await?;
                let length = channel.stat(&remote).await?.size_bytes;
                Ok::<_, String>((index, length))
            });
        }
        while let Some(result) = tasks.join_next().await {
            let (index, length) = result??;
            println!("PARALLEL_FILE index={index} bytes={length}");
            if length != size {
                return Err("parallel upload size mismatch".into());
            }
        }
        println!(
            "PARALLEL_DONE bytes={} seconds={:.1} mibps={:.1}",
            size * channels.len() as u64,
            started.elapsed().as_secs_f64(),
            size as f64 * channels.len() as f64 / started.elapsed().as_secs_f64() / 1048576.0
        );
        std::fs::remove_file(local)?;
    }
    drop(password);
    if mode == "smoke" || mode == "large" {
        let size = if mode == "smoke" {
            64 * 1024 * 1024
        } else {
            5_000_000_000u64
        };
        let local = local_dir.join(format!("source-{size}.bin"));
        std::fs::File::create(&local)?.set_len(size)?;
        let remote = format!("{root}/source-{size}.bin");
        let control = Arc::new(AtomicU8::new(RUNNING));
        let started = Instant::now();
        let first = upload_resumable(
            sftp.as_ref(),
            &local,
            &remote,
            "bench",
            &meter(
                control.clone(),
                if mode == "large" { 1_000_000_000 } else { 0 },
            ),
            &control,
        )
        .await;
        if mode == "large" {
            println!(
                "PAUSE result={first:?} seconds={:.1} part_bytes={}",
                started.elapsed().as_secs_f64(),
                sftp.stat(&format!("{remote}.part")).await?.size_bytes
            );
            control.store(RUNNING, Ordering::SeqCst);
            upload_resumable(
                sftp.as_ref(),
                &local,
                &remote,
                "bench",
                &meter(control.clone(), 0),
                &control,
            )
            .await?;
        } else {
            first?;
        }
        println!(
            "UPLOAD_DONE bytes={} seconds={:.1} mibps={:.1}",
            sftp.stat(&remote).await?.size_bytes,
            started.elapsed().as_secs_f64(),
            size as f64 / started.elapsed().as_secs_f64() / 1048576.0
        );
        let downloaded = local_dir.join(format!("download-{size}.bin"));
        let started = Instant::now();
        let first_download = download_resumable(
            sftp.as_ref(),
            &remote,
            &downloaded,
            "bench",
            &meter(
                control.clone(),
                if mode == "smoke" { 16 * 1024 * 1024 } else { 0 },
            ),
            &control,
        )
        .await;
        if mode == "smoke" {
            println!("DOWNLOAD_PAUSE result={first_download:?}");
            control.store(RUNNING, Ordering::SeqCst);
            download_resumable(
                sftp.as_ref(),
                &remote,
                &downloaded,
                "bench",
                &meter(control.clone(), 0),
                &control,
            )
            .await?;
        } else {
            first_download?;
        }
        println!(
            "DOWNLOAD_DONE bytes={} seconds={:.1} mibps={:.1}",
            std::fs::metadata(&downloaded)?.len(),
            started.elapsed().as_secs_f64(),
            size as f64 / started.elapsed().as_secs_f64() / 1048576.0
        );
        std::fs::remove_file(downloaded)?;
        std::fs::remove_file(local)?;
        sftp.remove_file(&remote).await?;
    }
    if mode == "small"
        || mode == "small-parallel"
        || mode == "small-download"
        || mode == "small-production"
    {
        let file_count: usize = std::env::var("TIDETERM_BENCH_FILES")
            .unwrap_or_else(|_| "10000".into())
            .parse()?;
        if !(1..=10_000).contains(&file_count) {
            return Err("benchmark file count must be 1..=10000".into());
        }
        println!("SMALL_READY sessions={}", channels.len());
        let small_dir = local_dir.join("small");
        std::fs::create_dir_all(&small_dir)?;
        let mut bytes = vec![0x5a; 25 * 1024];
        for index in 0..file_count {
            bytes[..8].copy_from_slice(&(index as u64).to_le_bytes());
            std::fs::write(small_dir.join(format!("{index:05}.bin")), &bytes)?;
        }
        let started = Instant::now();
        let mut next = 0usize;
        let mut tasks = tokio::task::JoinSet::new();
        while next < file_count || !tasks.is_empty() {
            while next < file_count && tasks.len() < channels.len() {
                let sftp = channels[next % channels.len()].clone();
                let source = small_dir.join(format!("{next:05}.bin"));
                let remote = format!("{root}/{next:05}.bin");
                tasks.spawn(async move {
                    let control = AtomicU8::new(RUNNING);
                    let sink = Quiet;
                    upload_resumable(sftp.as_ref(), &source, &remote, "bench", &sink, &control)
                        .await
                });
                next += 1;
            }
            tasks.join_next().await.unwrap()??;
            if next.is_multiple_of(1000) && tasks.len() + 1 == channels.len() {
                println!(
                    "SMALL_PROGRESS queued={next} seconds={:.1}",
                    started.elapsed().as_secs_f64()
                );
            }
        }
        println!(
            "SMALL_DONE count={file_count} bytes={} seconds={:.1} mibps={:.1}",
            file_count * 25 * 1024,
            started.elapsed().as_secs_f64(),
            (file_count * 25 * 1024) as f64 / started.elapsed().as_secs_f64() / 1048576.0
        );
        let entries = sftp.list_dir(&root).await?;
        let valid = entries
            .iter()
            .filter(|e| !e.is_dir && e.size_bytes == 25 * 1024)
            .count();
        println!("SMALL_VERIFY entries={} valid_sizes={valid}", entries.len());
        if entries.len() != file_count || valid != file_count {
            return Err("small upload count or size mismatch".into());
        }
        if mode == "small-download" {
            let downloads = local_dir.join("small-downloaded");
            std::fs::create_dir_all(&downloads)?;
            let started = Instant::now();
            let mut next = 0usize;
            let mut tasks = tokio::task::JoinSet::new();
            while next < file_count || !tasks.is_empty() {
                while next < file_count && tasks.len() < channels.len() {
                    let channel = channels[next % channels.len()].clone();
                    let remote = format!("{root}/{next:05}.bin");
                    let local = downloads.join(format!("{next:05}.bin"));
                    tasks.spawn(async move {
                        download_resumable(
                            channel.as_ref(),
                            &remote,
                            &local,
                            "bench",
                            &Quiet,
                            &AtomicU8::new(RUNNING),
                        )
                        .await
                    });
                    next += 1;
                }
                tasks.join_next().await.unwrap()??;
            }
            println!(
                "SMALL_DOWNLOAD_DONE count={file_count} seconds={:.1} mibps={:.1}",
                started.elapsed().as_secs_f64(),
                (file_count * 25 * 1024) as f64 / started.elapsed().as_secs_f64() / 1048576.0
            );
            for index in 0..file_count {
                let path = downloads.join(format!("{index:05}.bin"));
                if std::fs::metadata(&path)?.len() != 25 * 1024 {
                    return Err("downloaded small file size mismatch".into());
                }
                std::fs::remove_file(path)?;
            }
            std::fs::remove_dir(downloads)?;
        }
        for index in 0..file_count {
            std::fs::remove_file(small_dir.join(format!("{index:05}.bin")))?;
        }
        std::fs::remove_dir(small_dir)?;
    }
    drop(sftp);
    if let Some(instance) = production_instance {
        instance.disconnect().await?;
    }
    session.set_blocking(true);
    let mut cleanup = session.channel_session()?;
    cleanup.exec(&format!("rm -rf -- '{root}'"))?;
    let mut discard = Vec::new();
    cleanup.read_to_end(&mut discard)?;
    cleanup.wait_close()?;
    std::fs::remove_dir(&local_dir)?;
    println!("DONE");
    Ok(())
}
