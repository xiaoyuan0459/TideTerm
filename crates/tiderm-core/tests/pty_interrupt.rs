//! Real local-shell cancellation; the output marker cannot match command echo.
#![cfg(windows)]

use std::sync::Arc;
use std::time::{Duration, Instant};
use tiderm_core::protocol::{pty_impl::PtyInstance, Protocol};
use tokio::sync::mpsc;

async fn wait_for(rx: &mut mpsc::Receiver<Vec<u8>>, marker: &str, seconds: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut output = Vec::new();
    while let Ok(Some(bytes)) = tokio::time::timeout_at(until.into(), rx.recv()).await {
        output.extend(bytes);
        if String::from_utf8_lossy(&output).contains(marker) {
            return true;
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real cmd/PowerShell/Git Bash processes; run explicitly on Windows"]
async fn ctrl_c_interrupts_running_local_shells() {
    // ConPTY raises CTRL_C_EVENT inside the console process group. A Git Bash
    // (MSYS) test host starts the test binary outside that console ownership, so
    // ETX cannot reach cmd/PowerShell even though the shipped GUI is unaffected.
    // Fail loudly instead of reporting a product defect from an invalid host.
    if std::env::var_os("MSYSTEM").is_some() {
        panic!(
            "run this ignored regression from a native Windows console (PowerShell or cmd), not Git Bash/MSYS"
        );
    }
    for shell in ["cmd", "powershell", "pwsh", "bash"] {
        let dir = tempfile::tempdir().unwrap();
        let (args, command, ready, after) = match shell {
            "cmd" => (
                vec!["/d"],
                "echo __BEGIN_%OS% & ping -n 30 127.0.0.1 >nul\r",
                "__BEGIN_Windows",
                "echo __RECOVER_%OS%\r",
            ),
            "bash" => (
                vec!["--noprofile", "--norc", "-i"],
                "unset HISTFILE; printf '__BEGIN_%s\\n' BASH; sleep 30\r",
                "__BEGIN_BASH",
                "printf '__RECOVER_%s\\n' BASH\r",
            ),
            _ => (
                vec![
                    "-NoLogo",
                    "-NoProfile",
                    "-NoExit",
                    "-Command",
                    "function prompt { 'QA> ' }; [Console]::WriteLine('QA'+'READY')",
                ],
                "[Console]::WriteLine('__BEGIN_'+'PWSH'); Start-Sleep 30\r",
                "__BEGIN_PWSH",
                "[Console]::WriteLine('__RECOVER_'+'PWSH')\r",
            ),
        };
        let args: Vec<String> = args.into_iter().map(String::from).collect();
        let pty = Arc::new(
            PtyInstance::spawn_with_options(
                160,
                30,
                shell,
                &args,
                dir.path().to_str(),
                "xterm-256color",
            )
            .unwrap(),
        );
        let (tx, mut rx) = mpsc::channel(64);
        let reader = pty.clone();
        let pump = tokio::spawn(async move {
            while let Ok(bytes) = reader.read().await {
                if bytes.is_empty() || tx.send(bytes).await.is_err() {
                    break;
                }
            }
        });
        let initial = if shell == "cmd" {
            ">"
        } else if shell == "bash" {
            "bash-"
        } else {
            "QAREADY"
        };
        let started = wait_for(&mut rx, initial, 20).await;
        if started {
            pty.write(command.as_bytes()).await.unwrap();
        }
        let running = started && wait_for(&mut rx, ready, 8).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let clock = Instant::now();
        if running {
            pty.write(b"\x03").await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            pty.write(after.as_bytes()).await.unwrap();
        }
        let expected = if shell == "cmd" {
            "__RECOVER_Windows"
        } else if shell == "bash" {
            "__RECOVER_BASH"
        } else {
            "__RECOVER_PWSH"
        };
        let recovered = running && wait_for(&mut rx, expected, 7).await;
        let recovered_in = clock.elapsed();
        pty.disconnect().await.unwrap();
        drop(rx);
        let _ = tokio::time::timeout(Duration::from_secs(3), pump).await;
        assert!(started && running, "{shell}: long command never started");
        assert!(
            recovered && recovered_in < Duration::from_secs(8),
            "{shell}: Ctrl+C did not recover within 8 seconds"
        );
        eprintln!("{shell}: Ctrl+C recovered in {recovered_in:?}");
    }
}
